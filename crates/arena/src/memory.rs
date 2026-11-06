//! The memory guard every run passes before it starts a player: the transposition tables of all its
//! players together must stay under half the RAM the machine has (`kalo_chess::pipeline::system_memory`).
//! Strict on purpose: no allowance
//! is made for anything else a player holds, and a machine whose RAM cannot be read refuses to run,
//! since a run that swaps or is killed partway is worse than one that never starts.

use kalo_chess::pipeline::system_memory;

/// Refuses `hash_mb` for `players` tables at once unless they fit under half the machine's RAM.
pub fn check(hash_mb: usize, players: usize) -> Result<(), String> {
    let memory = system_memory::read().map_err(|error| format!("cannot check the tables fit in memory: {error}"))?;
    fits(hash_mb, players, memory.total_bytes / 1024)
}

/// The rule itself, on explicit values: `hash_mb * players < total / 2`.
fn fits(hash_mb: usize, players: usize, total_kb: u64) -> Result<(), String> {
    let tables_kb = hash_mb as u64 * 1024 * players as u64;
    let limit_kb = total_kb / 2;
    if tables_kb < limit_kb {
        return Ok(());
    }
    Err(format!(
        "{players} players x {hash_mb} MB of hash = {} MB, not under half the machine's {} MB of RAM ({} MB): lower --hash",
        tables_kb / 1024,
        total_kb / 1024,
        limit_kb / 1024
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tables_must_stay_strictly_under_half_the_ram() {
        let eight_gb_kb = 8 * 1024 * 1024;
        assert!(fits(16, 32, eight_gb_kb).is_ok(), "512 MB of 8 GB");
        assert!(fits(64, 64, eight_gb_kb).is_err(), "exactly 4 GB is not under half");
        assert!(fits(63, 64, eight_gb_kb).is_ok());
        let refused = fits(1024, 8, eight_gb_kb).unwrap_err();
        assert!(refused.contains("8192 MB") && refused.contains("lower --hash"), "{refused}");
    }

    /// The machine the tests run on can hold two default tables.
    #[test]
    fn this_machine_holds_two_default_tables() {
        assert!(check(16, 2).is_ok());
    }
}
