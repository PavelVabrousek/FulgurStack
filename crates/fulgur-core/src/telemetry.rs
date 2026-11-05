//! Platform telemetry; unavailable measurements remain null, never estimated.

/// Opt-in stage timeline for performance work (ADR-144): with the variable
/// `FULGURSTACK_TIMELINE` set, prints the seconds since the first mark and the
/// label to standard error. Without it, nothing is printed or measured.
pub fn timeline(label: &str) {
    static START: std::sync::OnceLock<Option<std::time::Instant>> = std::sync::OnceLock::new();
    let start = START.get_or_init(|| {
        std::env::var_os("FULGURSTACK_TIMELINE").map(|_| std::time::Instant::now())
    });
    if let Some(start) = start {
        eprintln!("timeline {:.3} {label}", start.elapsed().as_secs_f64());
    }
}

/// Physical memory only; no swap, process enumeration or identifying system data.
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct SystemMemory {
    pub total_bytes: u64,
    pub available_bytes: u64,
}

/// Process-wide user/kernel CPU time, including every application thread.
#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub struct ProcessCpuTime {
    pub user_seconds: f64,
    pub kernel_seconds: f64,
}

impl ProcessCpuTime {
    pub fn since(self, earlier: Self) -> Self {
        Self {
            user_seconds: (self.user_seconds - earlier.user_seconds).max(0.),
            kernel_seconds: (self.kernel_seconds - earlier.kernel_seconds).max(0.),
        }
    }
    pub fn total(self) -> f64 {
        self.user_seconds + self.kernel_seconds
    }
}

#[cfg(windows)]
pub fn process_cpu_time() -> Option<ProcessCpuTime> {
    use windows_sys::Win32::{
        Foundation::FILETIME,
        System::Threading::{GetCurrentProcess, GetProcessTimes},
    };
    let mut created: FILETIME = unsafe { std::mem::zeroed() };
    let mut exited = created;
    let mut kernel = created;
    let mut user = created;
    // SAFETY: all FILETIME destinations are initialized writable stack values.
    let ok = unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )
    };
    let seconds = |v: FILETIME| {
        ((u64::from(v.dwHighDateTime) << 32) | u64::from(v.dwLowDateTime)) as f64 * 1e-7
    };
    (ok != 0).then(|| ProcessCpuTime {
        user_seconds: seconds(user),
        kernel_seconds: seconds(kernel),
    })
}

#[cfg(not(windows))]
pub fn process_cpu_time() -> Option<ProcessCpuTime> {
    None
}

pub fn system_memory() -> Option<SystemMemory> {
    let mut system = sysinfo::System::new();
    system.refresh_memory_specifics(sysinfo::MemoryRefreshKind::nothing().with_ram());
    let total_bytes = system.total_memory();
    let available_bytes = system.available_memory();
    (total_bytes > 0 && available_bytes <= total_bytes).then_some(SystemMemory {
        total_bytes,
        available_bytes,
    })
}

#[cfg(all(test, any(windows, target_os = "macos")))]
mod tests {
    #[test]
    fn physical_memory_is_measured_on_supported_platforms() {
        let memory = super::system_memory().expect("Physical RAM query must work on Windows/macOS");
        assert!(memory.total_bytes > 0);
        assert!(memory.available_bytes <= memory.total_bytes);
    }

    #[test]
    #[cfg(windows)]
    fn process_cpu_counters_are_monotonic_and_have_truthful_deltas() {
        let before = super::process_cpu_time().unwrap();
        let mut value = 0_u64;
        for i in 0..1_000_000 {
            value = std::hint::black_box(value.wrapping_add(i));
        }
        std::hint::black_box(value);
        let after = super::process_cpu_time().unwrap();
        assert!(
            after.user_seconds >= before.user_seconds
                && after.kernel_seconds >= before.kernel_seconds
        );
        let delta = after.since(before);
        assert!((delta.total() - (after.total() - before.total())).abs() < 1e-6);
    }
}

#[cfg(windows)]
pub fn peak_rss_bytes() -> Option<u64> {
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    let mut counters: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
    counters.cb = size;
    // SAFETY: counters is initialized, correctly sized, and writable for the call.
    let ok = unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, size) };
    (ok != 0).then_some(counters.PeakWorkingSetSize as u64)
}

#[cfg(not(windows))]
pub fn peak_rss_bytes() -> Option<u64> {
    None
}
