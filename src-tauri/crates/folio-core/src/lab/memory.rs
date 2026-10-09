//! Peak memory of one process, read from the operating system.
//!
//! These are lifetime peaks of a single process at the moment they are read.
//! They are not per-task figures and never the whole device's memory. A reading
//! that cannot be made is `None` with a reason; nothing is estimated.

use crate::lab::record::{MemoryEntry, MemoryProcess};

#[derive(Clone, Debug, PartialEq)]
pub struct PeakReading {
    pub peak_bytes: Option<u64>,
    pub method: String,
    pub unavailable_reason: Option<String>,
}

impl PeakReading {
    fn measured(peak_bytes: u64, method: &str) -> Self {
        Self {
            peak_bytes: Some(peak_bytes),
            method: method.to_string(),
            unavailable_reason: None,
        }
    }

    fn unavailable(method: &str, reason: impl Into<String>) -> Self {
        Self {
            peak_bytes: None,
            method: method.to_string(),
            unavailable_reason: Some(reason.into()),
        }
    }

    /// A record entry naming the process and the span the peak covers.
    pub fn into_entry(self, process: MemoryProcess, pid: Option<u32>, scope: &str) -> MemoryEntry {
        MemoryEntry {
            process,
            pid,
            peak_bytes: self.peak_bytes,
            scope: scope.to_string(),
            method: self.method,
            unavailable_reason: self.unavailable_reason,
        }
    }
}

/// Peak memory of the process with this id.
pub fn process_peak(pid: u32) -> PeakReading {
    platform::process_peak(pid)
}

/// Peak memory of Folio's own process.
pub fn self_peak() -> PeakReading {
    process_peak(std::process::id())
}

/// `VmHWM` of a Linux `/proc/<pid>/status`, in bytes.
pub fn parse_vm_hwm(status: &str) -> Option<u64> {
    let line = status.lines().find(|line| line.starts_with("VmHWM:"))?;
    let mut parts = line["VmHWM:".len()..].split_whitespace();
    let value: u64 = parts.next()?.parse().ok()?;
    match parts.next() {
        Some("kB") => value.checked_mul(1024),
        _ => None,
    }
}

#[cfg(windows)]
mod platform {
    use super::PeakReading;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
    };

    const METHOD: &str = "GetProcessMemoryInfo PeakWorkingSetSize (peak working set)";

    pub fn process_peak(pid: u32) -> PeakReading {
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, 0, pid);
            if handle.is_null() {
                return PeakReading::unavailable(
                    METHOD,
                    format!(
                        "the process could not be opened: {}",
                        std::io::Error::last_os_error()
                    ),
                );
            }
            let mut counters = PROCESS_MEMORY_COUNTERS::default();
            counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
            let ok = GetProcessMemoryInfo(handle, &mut counters, counters.cb);
            let error = std::io::Error::last_os_error();
            CloseHandle(handle);
            if ok == 0 {
                return PeakReading::unavailable(
                    METHOD,
                    format!("memory counters could not be read: {error}"),
                );
            }
            PeakReading::measured(counters.PeakWorkingSetSize as u64, METHOD)
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::PeakReading;

    // The physical footprint leaves out clean file-backed pages, and
    // llama-server maps the model file by default, so the weights are not in
    // this figure. It can't be compared with the Windows and Linux peaks, which
    // count mapped pages the process touched; the method says so.
    const METHOD: &str = "proc_pid_rusage RUSAGE_INFO_V4 ri_lifetime_max_phys_footprint (peak physical footprint; excludes the memory-mapped model file)";

    pub fn process_peak(pid: u32) -> PeakReading {
        let Ok(pid) = i32::try_from(pid) else {
            return PeakReading::unavailable(METHOD, "the process id does not fit the system call");
        };
        unsafe {
            let mut info: libc::rusage_info_v4 = std::mem::zeroed();
            let status = libc::proc_pid_rusage(
                pid,
                libc::RUSAGE_INFO_V4,
                &mut info as *mut libc::rusage_info_v4 as *mut libc::rusage_info_t,
            );
            if status != 0 {
                return PeakReading::unavailable(
                    METHOD,
                    format!(
                        "the usage could not be read: {}",
                        std::io::Error::last_os_error()
                    ),
                );
            }
            PeakReading::measured(info.ri_lifetime_max_phys_footprint, METHOD)
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::{parse_vm_hwm, PeakReading};

    const METHOD: &str = "/proc/<pid>/status VmHWM (peak resident set)";

    pub fn process_peak(pid: u32) -> PeakReading {
        match std::fs::read_to_string(format!("/proc/{pid}/status")) {
            Err(error) => {
                PeakReading::unavailable(METHOD, format!("the status could not be read: {error}"))
            }
            Ok(status) => match parse_vm_hwm(&status) {
                Some(bytes) => PeakReading::measured(bytes, METHOD),
                None => PeakReading::unavailable(METHOD, "the status has no VmHWM line"),
            },
        }
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
mod platform {
    use super::PeakReading;

    pub fn process_peak(_pid: u32) -> PeakReading {
        PeakReading::unavailable(
            "none",
            "peak process memory is not read on this operating system",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vm_hwm_is_read_in_bytes() {
        let status =
            "Name:\tllama-server\nVmPeak:\t  900 kB\nVmHWM:\t  2048 kB\nVmRSS:\t 1024 kB\n";
        assert_eq!(parse_vm_hwm(status), Some(2048 * 1024));
    }

    #[test]
    fn a_missing_or_malformed_vm_hwm_is_not_guessed() {
        assert_eq!(parse_vm_hwm("Name:\tx\nVmRSS:\t1 kB\n"), None);
        assert_eq!(parse_vm_hwm("VmHWM:\tmany kB\n"), None);
        assert_eq!(parse_vm_hwm("VmHWM:\t12 MB\n"), None);
        assert_eq!(parse_vm_hwm("VmHWM:\t12\n"), None);
    }

    #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
    #[test]
    fn the_running_test_process_has_a_measured_peak() {
        let reading = self_peak();
        assert!(
            reading.peak_bytes.is_some_and(|bytes| bytes > 0),
            "{reading:?}"
        );
        assert!(reading.unavailable_reason.is_none());
        assert!(!reading.method.is_empty());
    }

    #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
    #[test]
    fn a_process_that_cannot_be_read_is_unavailable_with_a_reason() {
        let reading = process_peak(u32::MAX);
        assert_eq!(reading.peak_bytes, None);
        assert!(reading
            .unavailable_reason
            .as_deref()
            .is_some_and(|reason| !reason.is_empty()));
    }

    #[test]
    fn an_entry_names_its_process_and_the_span_it_covers() {
        let entry = PeakReading::unavailable("m", "why").into_entry(
            MemoryProcess::LlamaServer,
            Some(7),
            "process lifetime since start",
        );
        assert_eq!(entry.process, MemoryProcess::LlamaServer);
        assert_eq!(entry.pid, Some(7));
        assert_eq!(entry.peak_bytes, None);
        assert_eq!(entry.unavailable_reason.as_deref(), Some("why"));
        assert_eq!(entry.scope, "process lifetime since start");
    }
}
