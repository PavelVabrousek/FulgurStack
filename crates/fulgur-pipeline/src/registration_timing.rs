//! Non-overlapping registration wall intervals; never sums of overlapping worker times.
use serde::Serialize;
use std::time::Instant;

#[derive(Debug, Serialize)]
pub struct Phase {
    pub name: &'static str,
    pub wall_seconds: f64,
}

#[derive(Debug, Serialize)]
pub struct PhaseProfile {
    pub total_seconds: f64,
    pub phases: Vec<Phase>,
}

#[derive(Debug, Serialize)]
pub struct RegistrationProfile {
    pub native: PhaseProfile,
    pub native_fit_details: NativeFitDetails,
    pub external: Option<PhaseProfile>,
}

#[derive(Debug, Default, Serialize)]
pub struct NativeFitDetails {
    /// Per-fit durations may overlap; they are not a partition of phase wall time.
    pub timing_scope: Option<&'static str>,
    pub walk_group_wall_seconds: f64,
    pub walk_task_elapsed_seconds: f64,
    pub session_bridge_geometry_seconds: f64,
    pub direct_master_matches_seconds: f64,
    pub photometry_seconds: f64,
    pub bookkeeping_seconds: f64,
    pub frame_fits: Vec<FrameFitTiming>,
}

#[derive(Debug, Serialize)]
pub struct FrameFitTiming {
    pub source_index: usize,
    pub wall_seconds: f64,
    pub method: Option<&'static str>,
}

pub(crate) struct Timer {
    start: Instant,
    last: Instant,
    phases: Vec<Phase>,
}

impl Timer {
    pub(crate) fn new() -> Self {
        let start = Instant::now();
        Self {
            start,
            last: start,
            phases: Vec::with_capacity(8),
        }
    }

    pub(crate) fn lap(&mut self, name: &'static str) {
        let now = Instant::now();
        self.record(name, now);
    }

    fn record(&mut self, name: &'static str, now: Instant) {
        self.phases.push(Phase {
            name,
            wall_seconds: now.duration_since(self.last).as_secs_f64(),
        });
        self.last = now;
    }

    pub(crate) fn finish(mut self) -> PhaseProfile {
        let end = Instant::now();
        self.record("finalize", end);
        PhaseProfile {
            total_seconds: end.duration_since(self.start).as_secs_f64(),
            phases: self.phases,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phases_partition_wall_time_and_preserve_order_without_worker_sums() {
        let mut timer = Timer::new();
        timer.lap("read");
        timer.lap("parallel_fits");
        let result = timer.finish();
        assert_eq!(
            result.phases.iter().map(|p| p.name).collect::<Vec<_>>(),
            ["read", "parallel_fits", "finalize"]
        );
        assert!(
            result
                .phases
                .iter()
                .all(|p| p.wall_seconds.is_finite() && p.wall_seconds >= 0.)
        );
        let sum: f64 = result.phases.iter().map(|p| p.wall_seconds).sum();
        assert!((sum - result.total_seconds).abs() < 1e-12);
        let empty = Timer::new().finish();
        assert_eq!(empty.phases.len(), 1);
        assert_eq!(empty.phases[0].wall_seconds, empty.total_seconds);
    }
}
