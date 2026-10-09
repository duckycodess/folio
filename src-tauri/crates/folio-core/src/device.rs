//! Read-only facts about this computer for model setup: the whole device's
//! physical RAM, and the free space on the disk that holds Folio's models.
//! Each returns `None` when the operating system doesn't say, so the UI shows
//! "unknown" instead of a guess. Neither is Folio's own process memory.

use std::path::Path;

/// Total physical RAM of the whole device, in bytes.
pub fn total_memory_bytes() -> Option<u64> {
    imp::total_memory_bytes().filter(|bytes| *bytes > 0)
}

/// Bytes free for this user on the disk that holds `path`. A folder not
/// created yet is measured at its nearest existing parent, where it will be.
pub fn available_disk_bytes(path: &Path) -> Option<u64> {
    let existing = path.ancestors().find(|ancestor| ancestor.is_dir())?;
    imp::available_disk_bytes(existing)
}

#[cfg(target_os = "macos")]
mod imp {
    use std::path::Path;

    pub fn total_memory_bytes() -> Option<u64> {
        let mut bytes: u64 = 0;
        let mut size = std::mem::size_of::<u64>();
        // SAFETY: `hw.memsize` is a NUL-terminated name, and `bytes` and `size`
        // describe a u64 buffer the call writes at most `size` bytes into.
        let status = unsafe {
            libc::sysctlbyname(
                c"hw.memsize".as_ptr(),
                (&mut bytes as *mut u64).cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        (status == 0 && size == std::mem::size_of::<u64>()).then_some(bytes)
    }

    pub fn available_disk_bytes(path: &Path) -> Option<u64> {
        super::unix_available_disk_bytes(path)
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use std::path::Path;

    /// `MemTotal` leaves out memory reserved by firmware, so it can read a
    /// little below the installed RAM. Linux isn't a target platform.
    pub fn total_memory_bytes() -> Option<u64> {
        let info = std::fs::read_to_string("/proc/meminfo").ok()?;
        let line = info.lines().find(|line| line.starts_with("MemTotal:"))?;
        let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
        kib.checked_mul(1024)
    }

    pub fn available_disk_bytes(path: &Path) -> Option<u64> {
        super::unix_available_disk_bytes(path)
    }
}

#[cfg(windows)]
mod imp {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    use windows_sys::Win32::System::SystemInformation::{
        GetPhysicallyInstalledSystemMemory, GlobalMemoryStatusEx, MEMORYSTATUSEX,
    };

    pub fn total_memory_bytes() -> Option<u64> {
        // The RAM installed in the computer, from its firmware tables.
        // GlobalMemoryStatusEx reports only what Windows can use, which leaves
        // out memory reserved for an integrated GPU, so an 8 GB laptop without
        // a dedicated GPU could look like 7 GB. It is the fallback.
        let mut kib: u64 = 0;
        // SAFETY: `kib` is a writable u64 the call fills in.
        if unsafe { GetPhysicallyInstalledSystemMemory(&mut kib) } != 0 && kib > 0 {
            return kib.checked_mul(1024);
        }
        // SAFETY: MEMORYSTATUSEX is plain data; dwLength must be set before the call.
        let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
        status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        // SAFETY: `status` is a valid, writable MEMORYSTATUSEX with dwLength set.
        let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
        (ok != 0).then_some(status.ullTotalPhys)
    }

    pub fn available_disk_bytes(path: &Path) -> Option<u64> {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut available: u64 = 0;
        // SAFETY: `wide` is NUL-terminated; the two unused outputs may be null.
        let ok = unsafe {
            GetDiskFreeSpaceExW(
                wide.as_ptr(),
                &mut available,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        (ok != 0).then_some(available)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
mod imp {
    use std::path::Path;

    pub fn total_memory_bytes() -> Option<u64> {
        None
    }

    pub fn available_disk_bytes(_path: &Path) -> Option<u64> {
        None
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn unix_available_disk_bytes(path: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let name = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: statvfs is plain data that the call fills in.
    let mut stats: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `name` is NUL-terminated and `stats` is writable.
    if unsafe { libc::statvfs(name.as_ptr(), &mut stats) } != 0 {
        return None;
    }
    // Blocks free to unprivileged users, in fragment-size units. The field
    // widths differ between platforms, hence the conversions.
    let blocks = u64::from(stats.f_bavail);
    let block_size = u64::try_from(stats.f_frsize).ok()?;
    blocks.checked_mul(block_size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(any(target_os = "macos", target_os = "linux", windows))]
    fn reports_the_device_ram() {
        let bytes = total_memory_bytes().expect("the OS reports physical RAM");
        // Any computer that runs the tests has more than 256 MB.
        assert!(bytes > 256 * 1024 * 1024, "{bytes}");
    }

    #[test]
    #[cfg(any(target_os = "macos", target_os = "linux", windows))]
    fn reports_free_space_for_an_existing_folder() {
        let dir = tempfile::tempdir().unwrap();
        assert!(available_disk_bytes(dir.path()).is_some());
    }

    #[test]
    #[cfg(any(target_os = "macos", target_os = "linux", windows))]
    fn a_folder_not_created_yet_is_measured_where_it_will_be() {
        let dir = tempfile::tempdir().unwrap();
        let later = dir.path().join("not-yet").join("models");
        assert!(available_disk_bytes(&later).is_some());
    }
}
