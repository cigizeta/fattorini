//! `temper-chess --lichess`: each Lichess game's engine pinned to its own physical cores.
//!
//! lichess-bot starts one engine process per game, all with the same `uci_options`, so nothing in
//! UCI tells two games apart; the flag comes on the command line (`engine_options` in lichess-bot's
//! `config.yml`), and the engine works out the rest by itself. The physical cores come from
//! Windows, fastest first (`EfficiencyClass`, for hybrid CPUs); they are split into groups of
//! whole cores, each holding at least `Threads` logical processors; and the engine reserves the
//! first free group, pinning its whole process there, so every search thread it spawns later is
//! pinned too. Which cores stay free for everything else follows from lichess-bot's `concurrency`
//! and `Threads`: two games of `Threads 4` on an SMT CPU take cores 0-3, and later groups are never
//! claimed. With every group reserved, the engine plays unpinned; it never refuses to play.
//!
//! A group is reserved by creating a named mutex, `Local\temper-chess-lichess-group-<n>`. Creation is
//! atomic in the kernel: of two engines creating one name at once, exactly one creates it and the
//! other gets `ERROR_ALREADY_EXISTS`, so two engines never share a group. The owner keeps its
//! handle open until the process exits, crash included, when Windows closes it and the group is
//! free again. Existence, never ownership: a mutex is owned by a thread, and one whose owning
//! thread ends is abandoned and claimable while its process still plays. `Local\` names are seen
//! within one login session, where lichess-bot runs all its engines.

/// One physical core: how fast Windows says it is (higher is faster, equal on a non-hybrid CPU)
/// and its logical processors, as an affinity mask.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Core {
    efficiency_class: u8,
    mask: usize,
}

/// The cores fastest first, ties in Windows' own numbering (by lowest logical processor).
fn fastest_first(mut cores: Vec<Core>) -> Vec<Core> {
    cores.sort_by_key(|core| (std::cmp::Reverse(core.efficiency_class), core.mask.trailing_zeros()));
    cores
}

/// The groups, in claiming order: consecutive whole cores of `cores`, each group as few as hold at
/// least `threads` logical processors. Counted core by core, since cores may differ (an Intel
/// hybrid's P-cores have SMT, its E-cores do not). Cores left over that cannot fill a whole group
/// form none.
fn groups(cores: &[Core], threads: u32) -> Vec<Vec<Core>> {
    let mut groups = Vec::new();
    let mut group = Vec::new();
    for &core in cores {
        group.push(core);
        if mask_of(&group).count_ones() >= threads.max(1) {
            groups.push(std::mem::take(&mut group));
        }
    }
    groups
}

/// The affinity mask of all of `group`'s logical processors.
fn mask_of(group: &[Core]) -> usize {
    group.iter().fold(0, |mask, core| mask | core.mask)
}

/// `group`'s cores in order, each as its logical processors and efficiency class, such as
/// `0-1 (class 1), 4-5 (class 1)`: what the `info string` reports, so a hybrid CPU's ranking can be
/// checked against which cores are which.
fn describe_group(group: &[Core]) -> String {
    let cores: Vec<String> =
        group.iter().map(|core| format!("{} (class {})", describe(core.mask), core.efficiency_class)).collect();
    cores.join(", ")
}

/// The logical processors of `mask`, as a list such as `0-3` or `0,2`.
fn describe(mask: usize) -> String {
    let cpus: Vec<u32> = (0..usize::BITS).filter(|cpu| mask & (1usize << cpu) != 0).collect();
    let mut parts = Vec::new();
    let mut start = 0;
    while start < cpus.len() {
        let mut end = start;
        while end + 1 < cpus.len() && cpus[end + 1] == cpus[end] + 1 {
            end += 1;
        }
        parts.push(if end == start { cpus[start].to_string() } else { format!("{}-{}", cpus[start], cpus[end]) });
        start = end + 1;
    }
    parts.join(",")
}

/// Reserves the first free group for `threads` search threads and pins this process to it. The
/// `info string` line that says what happened: the group and its logical processors, or why the
/// engine plays unpinned.
pub fn pin(threads: u32) -> String {
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, SetProcessAffinityMask};

    let cores = match physical_cores() {
        Ok(cores) => fastest_first(cores),
        Err(reason) => return format!("info string --lichess: {reason}, playing unpinned"),
    };
    let groups = groups(&cores, threads);
    for (index, group) in groups.iter().enumerate() {
        if !reserve(index) {
            continue;
        }
        // SAFETY: `GetCurrentProcess` is a pseudo-handle always valid for this process.
        if unsafe { SetProcessAffinityMask(GetCurrentProcess(), mask_of(group)) } == 0 {
            let error = std::io::Error::last_os_error();
            return format!("info string --lichess: pinning to group {index} failed ({error}), playing unpinned");
        }
        return format!("info string --lichess: pinned to core group {index}, logical processors {}", describe_group(group));
    }
    format!("info string --lichess: all {} core groups are reserved, playing unpinned", groups.len())
}

/// Reserves group `index` for this process, by creating its named mutex. False when another
/// process holds it, or the mutex cannot be created. A held handle is never closed: Windows closes
/// it when the process exits, which frees the group.
fn reserve(index: usize) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS};
    use windows_sys::Win32::System::Threading::CreateMutexW;

    let name: Vec<u16> = format!("Local\\temper-chess-lichess-group-{index}").encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: `name` is a NUL-terminated UTF-16 string that outlives the call; no attributes.
    let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
    if handle.is_null() {
        return false;
    }
    // Read at once, before any other call can overwrite it.
    // SAFETY: no preconditions.
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        // Closed at once: a handle kept open would keep the mutex, and so the group, alive after
        // its owner exits.
        // SAFETY: `handle` is a valid handle this process opened and has not closed.
        unsafe { CloseHandle(handle) };
        return false;
    }
    true
}

/// Every physical core from `GetLogicalProcessorInformationEx(RelationProcessorCore)`, with its
/// `EfficiencyClass`. An error when the topology cannot be read, or spans more than one processor
/// group (over 64 logical processors), which one process affinity mask cannot cover.
fn physical_cores() -> Result<Vec<Core>, String> {
    use windows_sys::Win32::System::SystemInformation::{
        GetLogicalProcessorInformationEx, RelationProcessorCore, GROUP_AFFINITY, SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
    };
    use windows_sys::Win32::System::Threading::GetActiveProcessorGroupCount;

    // SAFETY: no preconditions.
    if unsafe { GetActiveProcessorGroupCount() } > 1 {
        return Err("more than one processor group (over 64 logical processors)".to_string());
    }
    let mut length: u32 = 0;
    // SAFETY: a null buffer with length 0 only asks for the length needed, written to `length`.
    unsafe { GetLogicalProcessorInformationEx(RelationProcessorCore, std::ptr::null_mut(), &mut length) };
    if length == 0 {
        return Err(format!("cannot read the processor topology ({})", std::io::Error::last_os_error()));
    }
    // u64 words, so the records, which hold pointer-sized masks, are suitably aligned.
    let mut buffer = vec![0u64; (length as usize).div_ceil(8)];
    let base = buffer.as_mut_ptr() as *mut SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX;
    // SAFETY: `buffer` holds at least `length` writable bytes.
    if unsafe { GetLogicalProcessorInformationEx(RelationProcessorCore, base, &mut length) } == 0 {
        return Err(format!("cannot read the processor topology ({})", std::io::Error::last_os_error()));
    }

    let mut cores = Vec::new();
    let mut offset = 0usize;
    while offset < length as usize {
        // SAFETY: the API wrote a sequence of records, each `Size` bytes long, within `length`;
        // every record asked for is a RelationProcessorCore, so `Processor` is the union's member.
        // With one processor group, checked above, each core has one `GroupMask`, group 0's.
        let (size, core) = unsafe {
            let record = (buffer.as_ptr() as *const u8).add(offset) as *const SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX;
            let processor = &(*record).Anonymous.Processor;
            let affinity: GROUP_AFFINITY = std::ptr::read_unaligned(std::ptr::addr_of!(processor.GroupMask) as *const GROUP_AFFINITY);
            ((*record).Size as usize, Core { efficiency_class: processor.EfficiencyClass, mask: affinity.Mask })
        };
        if size == 0 {
            break;
        }
        if core.mask != 0 {
            cores.push(core);
        }
        offset += size;
    }
    if cores.is_empty() {
        return Err("no processor core reported".to_string());
    }
    Ok(cores)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn core(efficiency_class: u8, mask: usize) -> Core {
        Core { efficiency_class, mask }
    }

    #[test]
    fn faster_cores_come_first_then_windows_numbering() {
        let cores = vec![core(0, 0b11 << 8), core(1, 0b11 << 2), core(0, 0b11 << 4), core(1, 0b11)];
        let ordered: Vec<usize> = fastest_first(cores).iter().map(|core| core.mask).collect();
        assert_eq!(ordered, vec![0b11, 0b11 << 2, 0b11 << 4, 0b11 << 8]);
    }

    fn masks(groups: &[Vec<Core>]) -> Vec<usize> {
        groups.iter().map(|group| mask_of(group)).collect()
    }

    #[test]
    fn four_threads_on_smt_cores_take_two_cores_each() {
        // 8 cores, 2 logical processors each.
        let cores: Vec<Core> = (0..8).map(|index| core(0, 0b11 << (2 * index))).collect();
        assert_eq!(masks(&groups(&cores, 4)), vec![0xf, 0xf0, 0xf00, 0xf000]);
    }

    #[test]
    fn groups_count_logical_processors_core_by_core() {
        // An Intel hybrid: two P-cores with SMT, then four E-cores without.
        let cores = vec![
            core(1, 0b11),
            core(1, 0b1100),
            core(0, 1 << 4),
            core(0, 1 << 5),
            core(0, 1 << 6),
            core(0, 1 << 7),
        ];
        assert_eq!(masks(&groups(&cores, 4)), vec![0xf, 0xf0]);
    }

    #[test]
    fn leftover_cores_form_no_group() {
        let cores: Vec<Core> = (0..3).map(|index| core(0, 0b11 << (2 * index))).collect();
        assert_eq!(masks(&groups(&cores, 4)), vec![0xf]);
    }

    #[test]
    fn zero_threads_still_take_one_core() {
        assert_eq!(masks(&groups(&[core(0, 0b11)], 0)), vec![0b11]);
    }

    #[test]
    fn a_group_reads_as_its_cores_and_their_classes() {
        assert_eq!(describe_group(&[core(1, 0b11), core(0, 0b11_0000)]), "0-1 (class 1), 4-5 (class 0)");
    }

    #[test]
    fn masks_read_as_processor_lists() {
        assert_eq!(describe(0xf), "0-3");
        assert_eq!(describe(0b101), "0,2");
        assert_eq!(describe(0b1101_0000), "4,6-7");
    }

    /// The machine the tests run on reports its cores, each with a logical processor.
    #[test]
    fn this_machine_has_physical_cores() {
        let cores = physical_cores().unwrap();
        assert!(cores.iter().all(|core| core.mask != 0));
        assert!(cores.len() <= std::thread::available_parallelism().unwrap().get());
    }
}
