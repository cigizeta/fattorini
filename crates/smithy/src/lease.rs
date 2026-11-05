//! A job's lease: any number of smithies may run, on any machines, and a job (a forge run, a tune,
//! PeSTO's play, Lc0's Convert, a source's test set being frozen) may stop in one and resume in
//! another, but never run in two at once.
//!
//! **Over HTTP alone, no file (the developer, 2026-11-01).** A lease is held in memory by the smithy
//! running the job, under its key: the path its `lease.txt` once had under the remote's root, the
//! same on every machine. Taking one asks every other machine of `machines.yaml` once, at once
//! (`JOBS_PATH`), which jobs its smithy is taking, holds, or released lately, and never asks again.
//! It is refused when another smithy takes or holds the job; when one released it under
//! `RELEASED_FOR` ago, so Drive has time to sync the files its stop wrote; and when one answers but
//! not readably, a build too old to ask. A machine that does not answer, twice, holds nothing: on a
//! home LAN it is off, or runs no smithy. Machines are never named. The job's own smithy is stopped
//! by hand to run it elsewhere: there is no take over.
//!
//! **Not a lock.** A machine that is on and holds the job, but fails to answer twice (a hung smithy,
//! a name lookup failing, a missing firewall rule), lets the job start a second time; nothing
//! detects it. Accepted.

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use temper_chess::pipeline::protocol::{JOBS_PATH, SMITHY_PORT};
use temper_chess::pipeline::remote;

/// The lease's former file in a job's remote directory, now the last part of its key; an old one
/// left there is ignored, and never copied (`tree.rs`, `tune.rs`).
pub(crate) const FILE: &str = "lease.txt";

/// How long another machine's smithy has to answer `JOBS_PATH`, at each of `ASK_ATTEMPTS`.
const ASK_TIMEOUT: Duration = Duration::from_secs(5);
const ASK_ATTEMPTS: usize = 2;

/// How long a job released is still said, so a resume elsewhere waits for Drive to sync the files
/// its stop wrote: about a minute's sync, twice.
const RELEASED_FOR: Duration = Duration::from_secs(120);

/// The machines asked when a lease is taken: this one's `data/machine.yaml` name and every machine of
/// `machines.yaml`. Set once by `smithy start`; unset (the tests), none is asked.
struct Peers {
    machine: String,
    machines: Vec<String>,
}

static PEERS: OnceLock<Peers> = OnceLock::new();

/// Sets the machines asked when a lease is taken: `machine` is this one, skipped.
pub(crate) fn set_peers(machine: &str, machines: &[String]) {
    PEERS.set(Peers { machine: machine.to_string(), machines: machines.to_vec() }).ok();
}

/// Where a job stands in this smithy.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    /// Its lease being taken: the other machines being asked.
    Taking,
    Held,
    /// Released at this instant, said until `RELEASED_FOR` has passed.
    Released(Instant),
}

/// This smithy's jobs, by key (`key_of`).
static JOBS: Mutex<Vec<(String, Phase)>> = Mutex::new(Vec::new());

fn jobs() -> std::sync::MutexGuard<'static, Vec<(String, Phase)>> {
    let mut jobs = JOBS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    jobs.retain(|(_, phase)| !matches!(phase, Phase::Released(at) if at.elapsed() >= RELEASED_FOR));
    jobs
}

/// A key as being taken, or a refusal if this smithy takes or holds it already. One it released
/// lately is taken again at once: its files are this machine's own.
fn begin(key: &str, job: &str) -> Result<(), String> {
    let mut jobs = jobs();
    if jobs.iter().any(|(held, phase)| held == key && !matches!(phase, Phase::Released(_))) {
        return Err(format!("{job} runs in this smithy already"));
    }
    jobs.retain(|(held, _)| held != key);
    jobs.push((key.to_string(), Phase::Taking));
    Ok(())
}

fn set(key: &str, phase: Phase) {
    for (held, at) in jobs().iter_mut() {
        if held == key {
            *at = phase;
        }
    }
}

fn forget(key: &str) {
    jobs().retain(|(held, _)| held != key);
}

/// A lease's key: its path under the remote's root, `/`-separated.
fn key_of(path: &str) -> String {
    let path = path.replace('\\', "/");
    let root = remote::this_root().replace('\\', "/");
    path.strip_prefix(&root).unwrap_or(&path).trim_start_matches('/').to_string()
}

/// `JOBS_PATH`'s answer after its `OK`: a line for each of this smithy's jobs.
pub(crate) fn jobs_text() -> String {
    let mut text = String::new();
    for (key, phase) in jobs().iter() {
        match phase {
            Phase::Taking => text.push_str(&format!("taking\t{key}\n")),
            Phase::Held => text.push_str(&format!("held\t{key}\n")),
            Phase::Released(at) => text.push_str(&format!("released\t{key}\t{}\n", at.elapsed().as_secs())),
        }
    }
    text
}

/// A job as another smithy said it.
#[derive(Clone, Debug, PartialEq)]
enum Said {
    Taking(String),
    Held(String),
    /// Released `seconds` ago.
    Released(String, u64),
}

/// `JOBS_PATH`'s answer, after its `OK`; `None` when unreadable.
fn parse_jobs(text: &str) -> Option<Vec<Said>> {
    let mut said = Vec::new();
    for line in text.lines().filter(|line| !line.is_empty()) {
        let fields: Vec<&str> = line.split('\t').collect();
        said.push(match fields.as_slice() {
            ["taking", key] => Said::Taking(key.to_string()),
            ["held", key] => Said::Held(key.to_string()),
            ["released", key, seconds] => Said::Released(key.to_string(), seconds.parse().ok()?),
            _ => return None,
        });
    }
    Some(said)
}

/// Another machine, as a lease being taken finds it.
#[derive(Clone, Debug, PartialEq)]
enum Peer {
    Answered(Vec<Said>),
    /// An answer, but not `OK` with readable jobs: a smithy too old to know `JOBS_PATH`.
    Unreadable,
    /// No answer at all, `ASK_ATTEMPTS` times: off, or no smithy.
    Silent,
}

/// Every other machine of `peers`, asked at once.
fn ask(peers: &Peers) -> Vec<Peer> {
    std::thread::scope(|scope| {
        let asked: Vec<_> = peers
            .machines
            .iter()
            .filter(|machine| **machine != peers.machine)
            .map(|machine| scope.spawn(move || ask_one(machine)))
            .collect();
        asked.into_iter().filter_map(|thread| thread.join().ok()).collect()
    })
}

fn ask_one(machine: &str) -> Peer {
    let url = lan_http::url(machine, SMITHY_PORT, JOBS_PATH);
    for _ in 0..ASK_ATTEMPTS {
        if let Ok(reply) = lan_http::get(&url, None, ASK_TIMEOUT) {
            return match lan_http::status_of(&reply) {
                Some((word, body)) if word == "OK" => {
                    parse_jobs(&String::from_utf8_lossy(&body)).map_or(Peer::Unreadable, Peer::Answered)
                }
                _ => Peer::Unreadable,
            };
        }
    }
    Peer::Silent
}

/// Whether `job`, its lease at `key`, may start here, given what the other machines said.
fn judge(job: &str, key: &str, peers: &[Peer]) -> Result<(), String> {
    for peer in peers {
        match peer {
            Peer::Unreadable => {
                return Err(format!("another machine's smithy is too old to ask whether it runs {job}: rebuild it"));
            }
            Peer::Answered(said) => {
                for said in said {
                    match said {
                        Said::Taking(held) | Said::Held(held) if held == key => {
                            return Err(format!("{job} runs in another smithy: it is not started here"));
                        }
                        Said::Released(held, seconds) if held == key && *seconds < RELEASED_FOR.as_secs() => {
                            return Err(format!(
                                "{job} stopped in another smithy {seconds} s ago; Drive may still be syncing its last files: resume it in a minute"
                            ));
                        }
                        _ => {}
                    }
                }
            }
            Peer::Silent => {}
        }
    }
    Ok(())
}

/// A lease held: released when dropped.
pub(crate) struct Lease {
    key: String,
}

/// Takes the lease at `path`, a job's former remote `lease.txt`, as `job`, the job's name in
/// messages, after asking the other machines (`judge`).
pub(crate) fn take(path: &str, job: &str) -> Result<Lease, String> {
    let peers = PEERS.get().map(ask).unwrap_or_default();
    take_as(&key_of(path), job, &peers)
}

fn take_as(key: &str, job: &str, peers: &[Peer]) -> Result<Lease, String> {
    begin(key, job)?;
    if let Err(message) = judge(job, key, peers) {
        forget(key);
        return Err(message);
    }
    set(key, Phase::Held);
    log::info!("{job}: lease taken");
    Ok(Lease { key: key.to_string() })
}

impl Drop for Lease {
    fn drop(&mut self) {
        set(&self.key, Phase::Released(Instant::now()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "data/x/lease.txt";

    fn answered(said: &[Said]) -> Peer {
        Peer::Answered(said.to_vec())
    }

    #[test]
    fn the_jobs_answer_round_trips_and_a_broken_one_is_none() {
        assert_eq!(
            parse_jobs("taking\tdata/a/lease.txt\nheld\tdata/b/lease.txt\nreleased\tdata/c/lease.txt\t42\n"),
            Some(vec![
                Said::Taking("data/a/lease.txt".to_string()),
                Said::Held("data/b/lease.txt".to_string()),
                Said::Released("data/c/lease.txt".to_string(), 42),
            ])
        );
        assert_eq!(parse_jobs(""), Some(Vec::new()), "no job");
        assert_eq!(parse_jobs("other\tdata/a\n"), None);
        assert_eq!(parse_jobs("released\tdata/a\tsoon\n"), None);
    }

    #[test]
    fn a_job_taken_or_held_elsewhere_is_refused() {
        assert!(judge("job", KEY, &[answered(&[Said::Held(KEY.to_string())])]).is_err());
        assert!(judge("job", KEY, &[answered(&[Said::Taking(KEY.to_string())])]).is_err());
        assert!(judge("job", KEY, &[answered(&[Said::Held("data/y/lease.txt".to_string())])]).is_ok(), "another job");
    }

    #[test]
    fn a_job_released_elsewhere_lately_is_refused_until_drive_has_synced() {
        assert!(judge("job", KEY, &[answered(&[Said::Released(KEY.to_string(), 30)])]).is_err());
        assert!(judge("job", KEY, &[answered(&[Said::Released(KEY.to_string(), 120)])]).is_ok());
    }

    #[test]
    fn a_silent_machine_holds_nothing_and_an_unreadable_one_refuses() {
        assert!(judge("job", KEY, &[Peer::Silent, answered(&[])]).is_ok());
        assert!(judge("job", KEY, &[Peer::Unreadable]).is_err(), "a build too old to ask");
        assert!(judge("job", KEY, &[]).is_ok(), "no other machine");
    }

    #[test]
    fn a_lease_is_held_until_dropped_then_said_released() {
        let key = "data/held/lease.txt";
        let lease = take_as(key, "job", &[]).unwrap();
        assert!(jobs_text().contains(&format!("held\t{key}\n")));
        assert!(take_as(key, "job", &[]).is_err(), "this smithy holds it already");
        drop(lease);
        assert!(jobs_text().contains(&format!("released\t{key}\t")));
        drop(take_as(key, "job", &[]).unwrap());
    }

    #[test]
    fn a_refused_lease_leaves_nothing_behind() {
        let key = "data/refused/lease.txt";
        assert!(take_as(key, "job", &[answered(&[Said::Held(key.to_string())])]).is_err());
        assert!(!jobs_text().contains(key));
    }
}
