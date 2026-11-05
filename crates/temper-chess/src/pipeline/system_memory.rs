//! The machine's physical memory, total and available, for the two budgets that depend on it:
//! `arena`'s memory guard on the transposition tables and `smithy`'s test cache. From
//! `GlobalMemoryStatusEx`.

/// Physical memory, in bytes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SystemMemory {
    pub total_bytes: u64,
    pub available_bytes: u64,
}

/// This machine's physical memory, or why it cannot be read.
pub fn read() -> Result<SystemMemory, String> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

    // SAFETY: a plain C struct, all zeros a valid value, whose size is set before the call as the
    // API requires.
    let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    // SAFETY: `status` is a valid, writable MEMORYSTATUSEX with its dwLength set.
    if unsafe { GlobalMemoryStatusEx(&mut status) } == 0 {
        return Err(format!("GlobalMemoryStatusEx failed: {}", std::io::Error::last_os_error()));
    }
    Ok(SystemMemory { total_bytes: status.ullTotalPhys, available_bytes: status.ullAvailPhys })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_machine_s_memory_is_readable() {
        let memory = read().unwrap();
        assert!(memory.total_bytes > 0);
        assert!(memory.available_bytes <= memory.total_bytes);
    }
}
