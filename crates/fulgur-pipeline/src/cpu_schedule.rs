//! Bounded long-lived CPU tasks: nested Rayon work cannot admit extra ROIs.
use fulgur_core::{Error, Result, check_cancelled};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Refill at most `workers` logical slots continuously, publishing in source order.
/// Completed values are current-tile samples; no extra source views are retained.
pub(crate) fn map_bounded<T: Send + Sync>(
    count: usize,
    workers: usize,
    cancel: &AtomicBool,
    compute: impl Fn(usize) -> Result<T> + Sync,
) -> Result<Vec<T>> {
    if !(1..=16).contains(&workers) {
        return Err(Error::Invalid("Invalid CPU reconstruction worker limit."));
    }
    check_cancelled(cancel)?;
    let next = AtomicUsize::new(0);
    let failed = AtomicBool::new(false);
    let slots: Vec<OnceLock<Result<T>>> = (0..count).map(|_| OnceLock::new()).collect();
    let compute = &compute;
    rayon::scope(|scope| {
        for _ in 0..workers.min(count) {
            let next = &next;
            let failed = &failed;
            let slots = &slots;
            scope.spawn(move |_| {
                while !failed.load(Ordering::Relaxed) && !cancel.load(Ordering::Relaxed) {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= count {
                        break;
                    }
                    let value = compute(index);
                    if value.is_err() {
                        failed.store(true, Ordering::Relaxed);
                    }
                    // The atomic index has exactly one owner. Each logical task
                    // finishes its entire ROI before claiming another index.
                    slots[index].set(value).unwrap_or_else(|_| unreachable!());
                }
            });
        }
    });
    // Join all work even after failure/cancellation, dropping every live ROI.
    check_cancelled(cancel)?;
    let mut output = Vec::with_capacity(count);
    let mut missing = false;
    for slot in slots {
        match slot.into_inner() {
            Some(value) => output.push(value?),
            None => missing = true,
        }
    }
    if missing {
        return Err(Error::Invalid("Incomplete CPU reconstruction results."));
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rayon::prelude::*;
    use std::sync::{Condvar, Mutex};
    use std::time::Duration;

    struct Active<'a>(&'a AtomicUsize);
    impl Drop for Active<'_> {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn continuous_workers_refill_before_first_group_finishes() {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .build()
            .unwrap();
        let progress = (Mutex::new(false), Condvar::new());
        let output = pool
            .install(|| {
                map_bounded(9, 2, &AtomicBool::new(false), |index| {
                    if index == 0 {
                        let started = progress.0.lock().unwrap();
                        let (started, _) = progress
                            .1
                            .wait_timeout_while(started, Duration::from_secs(5), |v| !*v)
                            .unwrap();
                        if !*started {
                            return Err(Error::Invalid(
                                "Next group did not start during straggler.",
                            ));
                        }
                    } else if index == 2 {
                        *progress.0.lock().unwrap() = true;
                        progress.1.notify_all();
                    }
                    Ok(index)
                })
            })
            .unwrap();
        assert_eq!(output, (0..9).collect::<Vec<_>>());
    }

    #[test]
    fn continuous_workers_bound_nested_work_and_preserve_every_source_bit() {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(8)
            .build()
            .unwrap();
        for workers in [1, 3, 16] {
            for count in [0, 1, 2, 17, 97] {
                let active = AtomicUsize::new(0);
                let peak = AtomicUsize::new(0);
                let visits: Vec<_> = (0..count).map(|_| AtomicUsize::new(0)).collect();
                let output = pool
                    .install(|| {
                        map_bounded(count, workers, &AtomicBool::new(false), |index| {
                            let resident = active.fetch_add(1, Ordering::SeqCst) + 1;
                            let _active = Active(&active);
                            peak.fetch_max(resident, Ordering::SeqCst);
                            visits[index].fetch_add(1, Ordering::Relaxed);
                            Ok((0..17)
                                .into_par_iter()
                                .map(|j| {
                                    let bits = (index as u32)
                                        .wrapping_mul(7919)
                                        .wrapping_add(j * 0x07000101);
                                    [
                                        f32::from_bits(bits),
                                        f32::from_bits(bits ^ 0x80000000),
                                        f32::from_bits(bits ^ 0x7fc00001),
                                    ]
                                })
                                .collect::<Vec<_>>())
                        })
                    })
                    .unwrap();
                assert!(peak.load(Ordering::SeqCst) <= workers.min(count));
                assert_eq!(active.load(Ordering::SeqCst), 0);
                for (index, pixels) in output.iter().enumerate() {
                    assert_eq!(visits[index].load(Ordering::Relaxed), 1);
                    for (j, pixel) in pixels.iter().enumerate() {
                        let bits = (index as u32)
                            .wrapping_mul(7919)
                            .wrapping_add(j as u32 * 0x07000101);
                        assert_eq!(
                            pixel.map(f32::to_bits),
                            [bits, bits ^ 0x80000000, bits ^ 0x7fc00001]
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn continuous_workers_join_and_release_on_error_and_cancellation() {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(4)
            .build()
            .unwrap();
        for cancellation in [false, true] {
            let cancel = AtomicBool::new(false);
            let active = AtomicUsize::new(0);
            let result = pool.install(|| {
                map_bounded(97, 4, &cancel, |index| {
                    active.fetch_add(1, Ordering::SeqCst);
                    let _active = Active(&active);
                    if index == 5 {
                        if cancellation {
                            cancel.store(true, Ordering::Relaxed);
                        } else {
                            return Err(Error::Invalid("Injected ROI failure."));
                        }
                    }
                    Ok(vec![index; 13])
                })
            });
            if cancellation {
                assert!(matches!(result, Err(Error::Cancelled)));
            } else {
                assert!(matches!(
                    result,
                    Err(Error::Invalid("Injected ROI failure."))
                ));
            }
            assert_eq!(active.load(Ordering::SeqCst), 0);
        }
        assert!(map_bounded(1, 0, &AtomicBool::new(false), Ok::<_, Error>).is_err());
        assert!(map_bounded(1, 17, &AtomicBool::new(false), Ok::<_, Error>).is_err());
        assert!(matches!(
            map_bounded(1, 1, &AtomicBool::new(true), |_| -> Result<()> {
                panic!("cancelled job started")
            }),
            Err(Error::Cancelled)
        ));
    }
}
