//! Which logical CPUs are separate physical cores, and pinning a thread to one of them.
//!
//! Timed pairs each run pinned to their own physical core: with no pondering and
//! `Threads 1`, only one engine of a game searches at any moment, so its two engines take turns on
//! one core, while two games on sibling hyperthreads would share one core's execution units and
//! distort each other's clocks.
//!
//! A Windows process inherits its parent's *process* affinity, never the spawning thread's:
//! `pin_child` pins each engine process explicitly, to the CPU its worker thread was pinned to.

/// One logical CPU per physical core, the lowest-numbered of each core's, sorted, from
/// `GetLogicalProcessorInformationEx(RelationProcessorCore)`. `None` when the topology cannot be
/// read, or spans more than one processor group (over 64 logical CPUs), which affinity masks here
/// do not handle: said on the console rather than silently pinning to the first group only.
pub fn physical_core_cpus() -> Option<Vec<usize>> {
    use windows_sys::Win32::System::SystemInformation::{
        GetLogicalProcessorInformationEx, RelationProcessorCore, SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
    };

    let mut length: u32 = 0;
    // SAFETY: a null buffer with length 0 only asks for the length needed, written to `length`.
    unsafe { GetLogicalProcessorInformationEx(RelationProcessorCore, std::ptr::null_mut(), &mut length) };
    if length == 0 {
        eprintln!("GetLogicalProcessorInformationEx: {}", std::io::Error::last_os_error());
        return None;
    }
    // u64 words, so the records, which hold pointer-sized masks, are suitably aligned.
    let mut buffer = vec![0u64; (length as usize).div_ceil(8)];
    let base = buffer.as_mut_ptr() as *mut SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX;
    // SAFETY: `buffer` holds at least `length` writable bytes.
    if unsafe { GetLogicalProcessorInformationEx(RelationProcessorCore, base, &mut length) } == 0 {
        eprintln!("GetLogicalProcessorInformationEx: {}", std::io::Error::last_os_error());
        return None;
    }

    let mut cores = Vec::new();
    let mut offset = 0usize;
    while offset < length as usize {
        // SAFETY: the API wrote a sequence of records, each `Size` bytes long, within `length`;
        // every record asked for is a RelationProcessorCore, so `Processor` is the union's member.
        // `GroupMask` is declared with one element but holds `GroupCount`, laid out in the record.
        let (size, groups) = unsafe {
            let record = (buffer.as_ptr() as *const u8).add(offset) as *const SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX;
            let processor = &(*record).Anonymous.Processor;
            let masks = std::ptr::addr_of!(processor.GroupMask) as *const windows_sys::Win32::System::SystemInformation::GROUP_AFFINITY;
            let groups: Vec<(u16, usize)> = (0..processor.GroupCount as usize)
                .map(|index| {
                    let affinity = std::ptr::read_unaligned(masks.add(index));
                    (affinity.Group, affinity.Mask)
                })
                .collect();
            ((*record).Size as usize, groups)
        };
        if size == 0 {
            break;
        }
        cores.push(groups);
        offset += size;
    }
    first_cpu_of_each_core_mask(&cores).map_err(|message| eprintln!("{message}")).ok()
}

/// The lowest CPU of each core from its group affinities, sorted. An error when any core lies
/// outside processor group 0, or none has a CPU.
fn first_cpu_of_each_core_mask(cores: &[Vec<(u16, usize)>]) -> Result<Vec<usize>, String> {
    let mut firsts = Vec::new();
    for groups in cores {
        if groups.iter().any(|(group, _)| *group != 0) {
            return Err("more than one processor group (over 64 logical CPUs): pinning is not supported".to_string());
        }
        let mask = groups.iter().fold(0usize, |mask, (_, group_mask)| mask | group_mask);
        if mask != 0 {
            firsts.push(mask.trailing_zeros() as usize);
        }
    }
    if firsts.is_empty() {
        return Err("no processor core reported".to_string());
    }
    firsts.sort_unstable();
    firsts.dedup();
    Ok(firsts)
}

thread_local! {
    /// The CPU `pin_current_thread` pinned this thread to, for `pin_child`.
    static PINNED_CPU: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

/// Pins the calling thread to `cpu`, and remembers it so `pin_child` pins the engine processes
/// this thread spawns afterwards, which Windows does not do by inheritance.
pub fn pin_current_thread(cpu: usize) -> Result<(), String> {
    use windows_sys::Win32::System::Threading::{GetCurrentThread, SetThreadAffinityMask};

    let mask = single_cpu_mask(cpu)?;
    // SAFETY: `GetCurrentThread` is a pseudo-handle always valid for the calling thread.
    if unsafe { SetThreadAffinityMask(GetCurrentThread(), mask) } == 0 {
        return Err(format!("SetThreadAffinityMask to CPU {cpu}: {}", std::io::Error::last_os_error()));
    }
    PINNED_CPU.with(|pinned| pinned.set(Some(cpu)));
    Ok(())
}

/// Pins `child` to the CPU this thread was pinned to, if any. Right after `spawn`: the child runs
/// unpinned for the moment before, harmless since no clock is running yet.
pub fn pin_child(child: &std::process::Child) -> Result<(), String> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::Threading::SetProcessAffinityMask;

    let Some(cpu) = PINNED_CPU.with(|pinned| pinned.get()) else {
        return Ok(());
    };
    let mask = single_cpu_mask(cpu)?;
    // SAFETY: `child`'s handle is a live process handle with full access, owned by `child`.
    if unsafe { SetProcessAffinityMask(child.as_raw_handle(), mask) } == 0 {
        return Err(format!("SetProcessAffinityMask to CPU {cpu}: {}", std::io::Error::last_os_error()));
    }
    Ok(())
}

/// The affinity mask with only `cpu` set.
fn single_cpu_mask(cpu: usize) -> Result<usize, String> {
    1usize.checked_shl(cpu as u32).ok_or(format!("CPU {cpu} is beyond the affinity mask"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_core_mask_gives_its_lowest_cpu() {
        // Siblings adjacent (0b11, 0b1100) and alone (0b10000), listed out of order.
        let cores = vec![vec![(0, 0b1100)], vec![(0, 0b11)], vec![(0, 0b10000)]];
        assert_eq!(first_cpu_of_each_core_mask(&cores), Ok(vec![0, 2, 4]));
    }

    #[test]
    fn a_second_processor_group_is_refused() {
        assert!(first_cpu_of_each_core_mask(&[vec![(0, 0b1)], vec![(1, 0b1)]]).is_err());
        assert!(first_cpu_of_each_core_mask(&[]).is_err());
    }

    /// The machine the tests run on reports its cores.
    #[test]
    fn this_machine_has_physical_cores() {
        let cpus = physical_core_cpus().unwrap();
        assert!(!cpus.is_empty());
        assert!(cpus.len() <= std::thread::available_parallelism().unwrap().get());
    }
}
