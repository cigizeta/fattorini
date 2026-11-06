//! `arena start|stop|status`: an arena, doing the smithy's chores (`smithy start`), over HTTP
//! (`temper_chess::pipeline::protocol`). Started by hand, it idles on `ARENA_PORT` until a smithy claims it,
//! then says hello to that smithy and plays the chores it is given, until stopped. It serves one
//! smithy at a time: another's claim is turned away while the claim is polled, and a claim no smithy
//! has polled for `RELEASE_AFTER` is released, the arena idle again. A chore is tasks alike but for
//! their openings, each drawn at random from the UHO book:
//!
//! - **Game:** one self-play game of the chore's player (`self_play::play_game_pgn`: MultiPV for the
//!   first plies, every move scored).
//! - **Pair:** the chore's two players on one opening with both colours, the challenger White first,
//!   no adjudication, as the gates play.
//!
//! Every worker plays the chore's tasks back to back and sends each as soon as it ends, as PGN, its
//! identity in its tags; the smithy's answer to it tops the chore up, stops it or replaces it. When
//! the chore runs out the arena asks for the next one. It knows nothing of jobs, generations, round
//! robins or gates: why a task is played is the smithy's business.
//!
//! A network is named by its file's path in the store the smithy's hello names, Drive (`G:`) for a
//! real smithy, and read from there: a network not there yet, still on its way up from the smithy's
//! machine, is waited for (`NETWORK_TIMEOUT`), and its chore given back when it never comes. A task
//! finished while its smithy cannot be reached is kept in an outbox, in memory, and sent once the same
//! start of the smithy claims the arena again; another start's claim drops it, its chores meaning
//! nothing there. An arena outlives its smithy: a restarted one claims it again. `start` runs in the
//! foreground; `stop` and `status`,
//! from another terminal on the same machine, reach it on `127.0.0.1:ARENA_CONTROL_PORT`, which also
//! keeps a second arena from starting beside it.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use temper_chess::nnue::Network;
use lan_http::{self as http, Listener, Reply, Request};
use temper_chess::pipeline::protocol::{
    self, parse_entries, task_body, Alive, Chore, Claim, Entry, Hello, HelloReply, InstallRequest, Limit, Player, TaskKind, ALIVE_PATH,
    ARENA_CONTROL_PORT, ARENA_PORT, CHORE_PATH, CHORE_TAG, CLAIM_PATH, HELLO_PATH, INSTALL_PATH, LOOPBACK, PESTO, RELEASE_AFTER,
    RELEASE_PATH, SEQ_TAG, SESSION_TAG, SMITHY_PORT, STATUS_PATH, STOP_PATH, TASK_PATH,
};
use temper_chess::tablebase::SyzygyTablebase;
use temper_chess::tt::TranspositionTable;

use super::config::ControlSpec;
use super::player::Control;
use super::in_process::InProcessPlayer;
use super::random_book::RandomBook;
use super::runner::{self, Recording};
use super::self_play::{play_game_pgn, TT_SIZE_MB};
use temper_chess::pipeline::spsa;

use super::{ctrlc_handler, data_root as arena_data_root, install, machine, pairing};

const PREFIX: &str = "arena";

/// How long an arena waits before asking again, after `WAIT` or an unreachable smithy.
const RETRY: Duration = Duration::from_secs(5);

/// How often the main loop and an idle worker look again at the chore.
const POLL: Duration = Duration::from_millis(200);

/// How long one request may take to send or answer.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// How long a network named by a chore is waited for in the store before the chore is given back: a
/// file just made on the smithy's machine takes a while to reach this one's Drive.
pub(crate) const NETWORK_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// How often the store is looked at again meanwhile.
pub(crate) const NETWORK_POLL: Duration = Duration::from_secs(5);

/// Each pair player's table, as the arena's default.
const PAIR_HASH_MB: usize = 16;

const USAGE: &str = "usage: arena start | stop | status";

fn fail(message: String) -> ! {
    eprintln!("{PREFIX}: {message}");
    std::process::exit(1);
}

/// `arena start|stop|status`, `verb` one of the three and `rest` empty.
pub(crate) fn run(verb: &str, rest: &[String]) {
    if !rest.is_empty() {
        fail(USAGE.to_string());
    }
    let none_running = "no arena is running on this machine";
    match verb {
        "start" => start(),
        "stop" => {
            http::control_request(ARENA_CONTROL_PORT, STOP_PATH, none_running).unwrap_or_else(|message| fail(message));
            println!("{PREFIX}: stop asked: the arena abandons the tasks in play, which the smithy never counts");
        }
        _ => print!("{}", http::control_request(ARENA_CONTROL_PORT, STATUS_PATH, none_running).unwrap_or_else(|message| fail(message))),
    }
}

fn start() {
    progress::log_events();
    let machine = machine::load(machine::MACHINE_CONFIG_PATH).unwrap_or_else(|message| fail(message));
    // Taken, the port means an arena already runs here: two would share the machine's cores.
    let control = Listener::bind(LOOPBACK, ARENA_CONTROL_PORT).unwrap_or_else(|message| {
        fail(format!("{message}: is an arena already running on this machine? arena status says"))
    });
    // Claimed over the LAN by a smithy on any machine, `127.0.0.1` kept for this machine's own.
    // Without the LAN's, only this machine's smithy can claim it.
    let mut claim_listeners = vec![Listener::bind(LOOPBACK, ARENA_PORT).unwrap_or_else(|message| fail(message))];
    match Listener::bind(&machine, ARENA_PORT) {
        Ok(listener) => claim_listeners.push(listener),
        Err(message) => log::warn!("{message}: claimed over {LOOPBACK} alone, by a smithy on this machine"),
    }
    let workers = std::thread::available_parallelism().map_or(1, |cores| cores.get());
    let hello = Hello { machine: machine.clone(), workers, commit: protocol::current_commit() };
    log::info!("{machine}, {} workers, at {}; waiting to be claimed by a smithy, on port {ARENA_PORT}", hello.workers, hello.commit);

    let stop = ctrlc_handler::install_ctrlc_handler();
    let arena = Arc::new(Arena {
        claim: Mutex::new(ClaimState::default()),
        hello,
        book: RandomBook::open(&arena_data_root::book_path()),
        tablebase: pairing::load_tablebase(),
        networks: Mutex::new(HashMap::new()),
        waiting_for: Mutex::new(HashSet::new()),
        speeds: Mutex::new(HashMap::new()),
        dealing: Mutex::new(Dealing::default()),
        install: Mutex::new(None),
        stop,
    });
    let control_done = Arc::new(AtomicBool::new(false));
    let mut threads = vec![{
        let (arena, control_done) = (Arc::clone(&arena), Arc::clone(&control_done));
        std::thread::spawn(move || control.serve(&|request: &Request| answer_control(&arena, request), &control_done))
    }];
    for listener in claim_listeners {
        let (arena, control_done) = (Arc::clone(&arena), Arc::clone(&control_done));
        threads.push(std::thread::spawn(move || listener.serve(&|request: &Request| answer_claim(&arena, request), &control_done)));
    }
    let result = arena.run();
    control_done.store(true, Ordering::SeqCst);
    for thread in threads {
        thread.join().ok();
    }
    match result {
        Ok(message) => log::info!("{message}"),
        Err(message) => fail(message),
    }
}

/// The control's answers, on `127.0.0.1` alone: the status, and a stop, as Ctrl+C.
fn answer_control(arena: &Arena, request: &Request) -> Reply {
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", STATUS_PATH) => Reply::status("OK", arena.status_text().as_bytes()),
        ("POST", STOP_PATH) => {
            arena.stop.store(true, Ordering::SeqCst);
            Reply::status("OK", b"")
        }
        _ => Reply::error(404, &format!("no {} {}", request.method, request.path)),
    }
}

/// The claim's answers, on `ARENA_PORT`: a smithy claiming the arena, polling it, releasing it, and
/// asking it to install.
fn answer_claim(arena: &Arc<Arena>, request: &Request) -> Reply {
    let body = String::from_utf8_lossy(&request.body).trim().to_string();
    match (request.method.as_str(), request.path.as_str()) {
        ("POST", CLAIM_PATH) => {
            let Ok(claim) = Claim::parse(&body) else {
                return Reply::error(400, "a malformed claim");
            };
            match arena.take_claim(claim, Instant::now()) {
                Ok(()) => Reply::status("OK", arena.hello.text().as_bytes()),
                Err(holder) => Reply::status("CLAIMED", holder.as_bytes()),
            }
        }
        ("POST", ALIVE_PATH) => {
            let outbox = {
                let mut state = arena.claim();
                match state.held.as_mut() {
                    Some((held, polled)) if held.boot == body => *polled = Instant::now(),
                    _ => return Reply::status("UNCLAIMED", b""),
                }
                state.outbox.len()
            };
            let install = match &*arena.install.lock().expect("the lock should not be poisoned") {
                None => "-".to_string(),
                Some(None) => "running".to_string(),
                Some(Some(line)) => line.clone(),
            };
            let installed = install::installed();
            let dealing = arena.dealing();
            let alive = Alive {
                session: dealing.joined.as_ref().map_or("-".to_string(), |(session, _, _)| session.clone()),
                chore: dealing.chore.as_ref().map_or("-".to_string(), |chore| chore.id.clone()),
                in_flight: dealing.in_flight,
                sent: dealing.sent,
                outbox,
                install,
                installed,
            };
            Reply::status("OK", alive.text().as_bytes())
        }
        ("POST", INSTALL_PATH) => {
            let Ok(asked) = InstallRequest::parse(&body) else {
                return Reply::error(400, "a malformed install request");
            };
            if arena.claimed().is_none_or(|claim| claim.boot != asked.boot) {
                return Reply::status("UNCLAIMED", b"");
            }
            let Some((_, _, store)) = arena.dealing().joined.clone() else {
                return Reply::status("UNJOINED", b"");
            };
            let job = match install::Install::of(&asked) {
                Ok(job) => job,
                Err(message) => return Reply::error(400, &message),
            };
            {
                let mut state = arena.install.lock().expect("the lock should not be poisoned");
                if matches!(*state, Some(None)) {
                    return Reply::status("BUSY", b"");
                }
                *state = Some(None);
            }
            let arena = Arc::clone(arena);
            std::thread::spawn(move || {
                log::info!("install of {} asked by the smithy", job.name());
                let line = match install::run(&job, &store, &arena.stop) {
                    Ok(line) => format!("done: {line}"),
                    Err(message) => format!("failed: {message}"),
                };
                log::info!("install: {line}");
                *arena.install.lock().expect("the lock should not be poisoned") = Some(Some(line));
            });
            Reply::status("OK", b"")
        }
        ("POST", RELEASE_PATH) => {
            let released = {
                let mut state = arena.claim();
                match state.held.take() {
                    Some((held, _)) if held.boot == body => Some(held.smithy),
                    other => {
                        state.held = other;
                        None
                    }
                }
            };
            if let Some(smithy) = released {
                arena.dealing().set_chore(None);
                log::info!("released by the smithy on {smithy}; waiting to be claimed");
            }
            Reply::status("OK", b"")
        }
        _ => Reply::error(404, &format!("no {} {}", request.method, request.path)),
    }
}

/// The smithy this arena serves, and what it keeps for it. Locked before `Dealing` when both are,
/// never while `Dealing` is held.
#[derive(Default)]
struct ClaimState {
    /// The claim held, and when it was last polled.
    held: Option<(Claim, Instant)>,
    /// The boot last served, kept after a release: a claim of the same boot carries on where it was,
    /// its session and its outbox kept.
    served: Option<String>,
    /// Tasks finished for the `served` boot and not sent: their token and body.
    outbox: Vec<(String, Vec<u8>)>,
}

impl ClaimState {
    /// Takes `claim`, saying whether it is of another boot than the one last served, which starts
    /// afresh (no session, the outbox dropped); or names the smithy holding the arena. A claim held is
    /// given up for another only when it is silent, when the other is a new start of the same smithy,
    /// or when the other outranks it (`Claim::outranks`): its smithy, polling next, finds the arena
    /// no longer its own and deals its work again.
    fn take(&mut self, claim: Claim, now: Instant) -> Result<bool, String> {
        if let Some((held, polled)) = &self.held {
            let silent = now.saturating_duration_since(*polled) > RELEASE_AFTER;
            if held.boot != claim.boot && held.smithy != claim.smithy && !silent && !claim.outranks(held) {
                return Err(held.standing());
            }
            if held.boot != claim.boot && held.smithy != claim.smithy && !silent {
                log::info!("the smithy on {} outranks the smithy on {}: this arena is its now", claim.smithy, held.smithy);
            }
        }
        let new_boot = self.served.as_deref() != Some(claim.boot.as_str());
        if new_boot {
            self.outbox.clear();
            self.served = Some(claim.boot.clone());
        }
        self.held = Some((claim, now));
        Ok(new_boot)
    }

    /// The claim, released when unpolled for `RELEASE_AFTER` by `now`: `Err` with it then.
    fn lapse(&mut self, now: Instant) -> Result<Option<Claim>, Claim> {
        if self.held.as_ref().is_some_and(|(_, polled)| now.saturating_duration_since(*polled) > RELEASE_AFTER) {
            return Err(self.held.take().expect("just checked").0);
        }
        Ok(self.held.as_ref().map(|(claim, _)| claim.clone()))
    }
}

/// The session and the chore its workers take tasks from.
#[derive(Default)]
struct Dealing {
    /// `(session, token, store)` once joined; `None` before, and after the smithy forgot the session.
    joined: Option<(String, String, String)>,
    chore: Option<Chore>,
    /// The next task's sequence number in the session.
    next_seq: u64,
    /// Tasks being played.
    in_flight: usize,
    /// Tasks sent this session, for the status.
    sent: u64,
    /// The last wait's reason, said once.
    waiting_said: Option<String>,
    /// Whether a worker has said the smithy is not answering its tasks: said once for all the
    /// workers, until one gets through.
    unreachable_said: bool,
}

impl Dealing {
    fn tasks_left(&self) -> u64 {
        self.chore.as_ref().map_or(0, |chore| chore.entries.iter().map(|entry| u64::from(entry.count)).sum())
    }

    /// Plays `chore` from now on, said in the log; its progress is on the smithy's page.
    fn set_chore(&mut self, chore: Option<Chore>) {
        if let Some(chore) = &chore {
            log::info!("{}", chore.summary());
        }
        self.chore = chore;
    }

    /// The smithy forgot this session, at a new job, or another start of it claimed the arena: hello
    /// again, the chore dropped.
    fn forget(&mut self) {
        self.joined = None;
        self.set_chore(None);
    }
}

/// One task to play, as a worker takes it.
struct Task {
    /// The boot of the smithy it was taken for: sent only to it.
    boot: String,
    session: String,
    token: String,
    /// Where the chore's networks are read from (`HelloReply::store`).
    store: String,
    chore: String,
    kind: TaskKind,
    limit: Limit,
    adjudicate: bool,
    reference_nps: Option<f64>,
    players: Vec<Player>,
    tags: Vec<(String, String)>,
    seq: u64,
}

struct Arena {
    claim: Mutex<ClaimState>,
    hello: Hello,
    book: RandomBook,
    tablebase: Arc<SyzygyTablebase>,
    /// The networks loaded, by their path in the store, kept while the chore names them.
    networks: Mutex<HashMap<String, Arc<Network>>>,
    /// The networks said to be waited for, so each is said once.
    waiting_for: Mutex<HashSet<String>>,
    /// This machine's loaded speed with each network a tune has named, by name (`loaded_bench`).
    speeds: Mutex<HashMap<String, f64>>,
    dealing: Mutex<Dealing>,
    /// The install, one at a time: `None` before any, `Some(None)` while one runs, `Some(Some(line))`
    /// once it has ended, saying how.
    install: Mutex<Option<Option<String>>>,
    stop: Arc<AtomicBool>,
}

impl Arena {
    fn dealing(&self) -> std::sync::MutexGuard<'_, Dealing> {
        self.dealing.lock().expect("the lock should not be poisoned")
    }

    fn claim(&self) -> std::sync::MutexGuard<'_, ClaimState> {
        self.claim.lock().expect("the lock should not be poisoned")
    }

    /// The claim held, if any.
    fn claimed(&self) -> Option<Claim> {
        return self.claim().held.as_ref().map(|(claim, _)| claim.clone());
    }

    /// Takes `claim` (`ClaimState::take`), or names the smithy holding the arena.
    fn take_claim(&self, claim: Claim, now: Instant) -> Result<(), String> {
        let (new_boot, said, dropped) = {
            let mut state = self.claim();
            let said = state.held.as_ref().is_none_or(|(held, _)| held.boot != claim.boot);
            let outbox = state.outbox.len();
            let new_boot = state.take(claim.clone(), now)?;
            (new_boot, said, if new_boot { outbox } else { 0 })
        };
        if dropped > 0 {
            log::info!("{dropped} tasks held for an earlier smithy dropped");
        }
        if new_boot {
            self.dealing().forget();
        }
        if said {
            log::info!("claimed by the smithy on {}, at {}", claim.smithy, claim.host);
        }
        Ok(())
    }

    /// The claim held, released first when no smithy has polled it for `RELEASE_AFTER`: then the
    /// chore goes, the workers finish their tasks into the outbox, and the arena idles.
    fn claim_held(&self) -> Option<Claim> {
        let lapsed = self.claim().lapse(Instant::now());
        match lapsed {
            Ok(held) => held,
            Err(claim) => {
                self.dealing().set_chore(None);
                log::info!("no word from the smithy on {} for {} s: released, waiting to be claimed", claim.smithy, RELEASE_AFTER.as_secs());
                None
            }
        }
    }

    /// Keeps a task that could not be sent, if it was played for the boot last served.
    fn keep(&self, task: &Task, body: Vec<u8>) {
        let mut state = self.claim();
        if state.served.as_deref() == Some(task.boot.as_str()) {
            state.outbox.push((task.token.clone(), body));
        }
    }

    /// Sends the outbox to `claim`'s smithy, keeping what still cannot be sent. Its answers are not
    /// applied: the chores they speak of are the smithy's to close or deal again.
    fn flush_outbox(&self, claim: &Claim) {
        let pending = std::mem::take(&mut self.claim().outbox);
        if pending.is_empty() {
            return;
        }
        let count = pending.len();
        let mut unsent = Vec::new();
        for (token, body) in pending {
            if !unsent.is_empty() || self.post(&claim.host, TASK_PATH, Some(&token), &body).is_err() {
                unsent.push((token, body));
            }
        }
        if unsent.len() < count {
            log::info!("{} tasks held while the smithy was away sent", count - unsent.len());
        }
        let mut state = self.claim();
        if state.served.as_deref() == Some(claim.boot.as_str()) {
            unsent.append(&mut state.outbox);
            state.outbox = unsent;
        }
    }

    fn status_text(&self) -> String {
        let (claim, outbox) = {
            let state = self.claim();
            (state.held.as_ref().map(|(claim, _)| claim.clone()), state.outbox.len())
        };
        let dealing = self.dealing();
        let standing = match &dealing.joined {
            Some((session, _, _)) => format!("joined as {session}, {} tasks sent", dealing.sent),
            None => "not joined".to_string(),
        };
        let playing = match &dealing.chore {
            Some(chore) => format!(
                "chore {}, {} of {}s at {}: {} left, {} in play",
                chore.id,
                chore.kind.name(),
                chore.kind.task(),
                chore.limit.text(),
                dealing.tasks_left(),
                dealing.in_flight
            ),
            None => "no chore".to_string(),
        };
        let smithy = match claim {
            Some(claim) => format!("claimed by the smithy on {} ({}), at {}, port {SMITHY_PORT}", claim.smithy, claim.standing(), claim.host),
            None => format!("waiting to be claimed, on port {ARENA_PORT}"),
        };
        format!("{smithy}: {standing}, {outbox} tasks held to send\nplaying {playing}\n")
    }

    /// A request's status word and body; an error only when the smithy could not be reached. An HTTP
    /// refusal reads as `HTTP <code>`, which no caller takes for an answer it knows.
    fn exchange(&self, reply: Result<Reply, String>) -> Result<(String, Vec<u8>), String> {
        let reply = reply?;
        Ok(http::status_of(&reply).unwrap_or_else(|| (format!("HTTP {}", reply.code), reply.body.clone())))
    }

    fn post(&self, host: &str, path: &str, token: Option<&str>, body: &[u8]) -> Result<(String, Vec<u8>), String> {
        self.exchange(http::post(&http::url(host, SMITHY_PORT, path), token, body, REQUEST_TIMEOUT))
    }

    /// Says hello to the smithy holding the claim and asks for chores, while the workers play them,
    /// until stopped (`Ok` with what to print) or given a chore this build cannot play (`Err`).
    /// Unclaimed, it idles. An unreachable smithy is asked again every `RETRY`, while its claim lasts.
    fn run(&self) -> Result<String, String> {
        let failure: Mutex<Option<String>> = Mutex::new(None);
        std::thread::scope(|scope| {
            for worker in 0..self.hello.workers {
                let failure = &failure;
                scope.spawn(move || {
                    if let Err(message) = self.work(worker) {
                        failure.lock().expect("the lock should not be poisoned").get_or_insert(message);
                        self.stop.store(true, Ordering::SeqCst);
                    }
                });
            }
            if let Err(message) = self.deal() {
                failure.lock().expect("the lock should not be poisoned").get_or_insert(message);
                self.stop.store(true, Ordering::SeqCst);
            }
        });
        match failure.into_inner().expect("no thread should have poisoned the lock") {
            Some(message) => Err(message),
            None => Ok("stopped: the tasks in play are never counted; start it again to join again".to_string()),
        }
    }

    /// The main loop: while claimed, the outbox sent, hello, and the next chore whenever the last one
    /// is played out.
    fn deal(&self) -> Result<(), String> {
        let mut unreachable_said = false;
        while !self.stop.load(Ordering::SeqCst) {
            let Some(claim) = self.claim_held() else {
                std::thread::sleep(POLL);
                continue;
            };
            self.flush_outbox(&claim);
            let (joined, idle) = {
                let dealing = self.dealing();
                (dealing.joined.clone(), dealing.tasks_left() == 0 && dealing.in_flight == 0)
            };
            let exchange = match &joined {
                None => self.post(&claim.host, HELLO_PATH, None, self.hello.text().as_bytes()),
                Some(_) if !idle => {
                    std::thread::sleep(POLL);
                    continue;
                }
                Some((session, token, _)) => self.post(&claim.host, CHORE_PATH, Some(token), session.as_bytes()),
            };
            let (status, body) = match exchange {
                Ok(reply) => {
                    unreachable_said = false;
                    reply
                }
                // The session kept: the smithy may be back before the claim lapses, its tasks taken.
                Err(message) => {
                    if !unreachable_said {
                        log::warn!("the smithy is not answering ({message}); asking again every {} s", RETRY.as_secs());
                        unreachable_said = true;
                    }
                    std::thread::sleep(RETRY);
                    continue;
                }
            };
            let text = String::from_utf8_lossy(&body).to_string();
            let mut dealing = self.dealing();
            match (status.as_str(), &joined) {
                // A restarted smithy, which forgot this session and holds a new token: hello again.
                ("TOKEN" | "UNKNOWN", _) => dealing.forget(),
                ("WAIT", _) => {
                    // Said again only when the reason changes: a wait between jobs may last hours.
                    let reason = text.trim().to_string();
                    if dealing.waiting_said.as_ref() != Some(&reason) {
                        log::info!("waiting: {}", if reason.is_empty() { "nothing to play yet" } else { &reason });
                        dealing.waiting_said = Some(reason);
                    }
                    drop(dealing);
                    std::thread::sleep(RETRY);
                }
                ("OK", None) => {
                    let reply = HelloReply::parse(&text)?;
                    log::info!("joined as {}, its networks read from {}", reply.session, reply.store);
                    // Tasks of an earlier session may still be in play: their ends are counted down.
                    let (in_flight, sent) = (dealing.in_flight, dealing.sent);
                    *dealing = Dealing { joined: Some((reply.session, reply.token, reply.store)), in_flight, sent, ..Dealing::default() };
                }
                ("CHORE", Some(_)) => {
                    let chore = Chore::parse(&text)?;
                    check(&chore)?;
                    dealing.waiting_said = None;
                    dealing.set_chore(Some(chore));
                }
                (other, _) => return Err(format!("the smithy answered {other:?}, which this build does not know")),
            }
        }
        Ok(())
    }

    /// The next task to play, its entry's count taken down, or `None` when there is none.
    fn take(&self) -> Option<Task> {
        // The claim read before `Dealing` is locked, never while it is.
        let boot = self.claimed()?.boot;
        let mut dealing = self.dealing();
        let (session, token, store) = dealing.joined.clone()?;
        let seq = dealing.next_seq;
        let chore = dealing.chore.as_mut()?;
        let index = next_entry(chore)?;
        let entry = &mut chore.entries[index];
        entry.count -= 1;
        let task = Task {
            boot,
            session,
            token,
            store,
            chore: chore.id.clone(),
            kind: chore.kind,
            limit: chore.limit.clone(),
            adjudicate: chore.adjudicate,
            reference_nps: chore.reference_nps,
            players: entry.players.clone(),
            tags: entry.tags.clone(),
            seq,
        };
        dealing.next_seq += 1;
        dealing.in_flight += 1;
        Some(task)
    }

    /// One worker: tasks of the chore, back to back, each sent as it ends.
    fn work(&self, worker: usize) -> Result<(), String> {
        let mut tt = TranspositionTable::new(TT_SIZE_MB);
        let mut rng = rand::rng();
        let mut reader = self.book.reader();
        while !self.stop.load(Ordering::SeqCst) {
            let Some(task) = self.take() else {
                std::thread::sleep(POLL);
                continue;
            };
            // Under a clock, half the logical cores play, as arena's matches do: a game per logical
            // core would cost the searches depth, and so move the results.
            if let Limit::Clock(_) = task.limit
                && worker >= (self.hello.workers / 2).max(1) {
                    self.give_back(&task);
                    std::thread::sleep(POLL);
                    continue;
                }
            let fen = self.book.random_fen(&mut reader, &mut rng);
            let played = match task.kind {
                TaskKind::SelfPlay => self.game(&task, &fen, &tt, &mut rng),
                TaskKind::Pair | TaskKind::Tournament | TaskKind::Spsa => self.pair(&task, worker, &fen),
            };
            tt.clear();
            let pgn = match played {
                Ok(Played::Pgn(pgn)) => pgn,
                Ok(Played::Stopped) => {
                    self.dealing().in_flight -= 1;
                    continue;
                }
                Ok(Played::Unavailable(network)) => {
                    self.drop_chore(&task, &network);
                    continue;
                }
                Err(message) => {
                    self.dealing().in_flight -= 1;
                    return Err(message);
                }
            };
            self.send(&task, &pgn);
            self.dealing().in_flight -= 1;
        }
        Ok(())
    }

    /// Gives up `task`'s chore, a network it names never having reached the store: its tasks are
    /// dropped, uncounted, and the next chore asked for once the workers are idle, the smithy dealing
    /// the dropped tasks to whichever arena asks.
    fn drop_chore(&self, task: &Task, network: &str) {
        let mut dealing = self.dealing();
        dealing.in_flight -= 1;
        if dealing.chore.as_ref().is_some_and(|chore| chore.id == task.chore) {
            log::warn!(
                "{network} is not in {} after {} minutes: chore {} given back",
                task.store,
                NETWORK_TIMEOUT.as_secs() / 60,
                task.chore
            );
            dealing.set_chore(None);
        }
    }

    /// A task taken and not played: its count back to its chore, if the chore is still the same.
    fn give_back(&self, task: &Task) {
        let mut dealing = self.dealing();
        dealing.in_flight -= 1;
        if let Some(chore) = dealing.chore.as_mut().filter(|chore| chore.id == task.chore)
            && let Some(entry) = chore.entries.iter_mut().find(|entry| entry.players == task.players && entry.tags == task.tags) {
                entry.count += 1;
            }
    }

    /// One self-play game, its PGN with the task's tags.
    fn game(&self, task: &Task, fen: &str, tt: &TranspositionTable, rng: &mut impl rand::RngExt) -> Result<Played, String> {
        let Limit::Depth(depth) = task.limit else {
            return Err(format!("chore {}: a game is played to a depth", task.chore));
        };
        let player = &task.players[0];
        let network = match self.network(&task.store, &player.network)? {
            Fetched::Ready(network) => network,
            Fetched::Stopped => return Ok(Played::Stopped),
            Fetched::Missing => return Ok(Played::Unavailable(player.network.clone())),
        };
        let Some((pgn, _)) = play_game_pgn(fen, depth, tt, &self.tablebase, network.as_deref(), player.parameters, &player.network, rng, &self.stop) else {
            return Ok(Played::Stopped);
        };
        Ok(Played::Pgn(with_tags(&pgn, &identity(task))))
    }

    /// One pair, both games' PGN with the task's tags.
    fn pair(&self, task: &Task, worker: usize, fen: &str) -> Result<Played, String> {
        let mut spec = match &task.limit {
            Limit::Depth(depth) => ControlSpec::depth(&depth.to_string()),
            Limit::Clock(clock) => ControlSpec::time(clock),
        }
        .map_err(|message| format!("chore {}: {message}", task.chore))?;
        let (challenger, teacher) = (&task.players[0], &task.players[1]);
        let mut networks = Vec::new();
        for player in [challenger, teacher] {
            match self.network(&task.store, &player.network)? {
                Fetched::Ready(network) => networks.push(network),
                Fetched::Stopped => return Ok(Played::Stopped),
                Fetched::Missing => return Ok(Played::Unavailable(player.network.clone())),
            }
        }
        let teacher_network = networks.pop().expect("two networks fetched");
        let challenger_network = networks.pop().expect("two networks fetched");
        let mut tags = identity(task);
        // A tune's clock, scaled to this machine's speed against the smithy's, so every game searches
        // about as many nodes wherever it is played.
        if let (Limit::Clock(_), Some(reference)) = (&task.limit, task.reference_nps) {
            let nps = self.speed(&challenger.network, challenger_network.as_deref());
            spec = spec.scaled(spsa::clock_factor(reference, nps));
            tags.push(("TimeControl".to_string(), spec.text.clone()));
        }
        let control: Control = spec.control;
        let mut challenger_player = InProcessPlayer::new(challenger_network, Arc::clone(&self.tablebase), PAIR_HASH_MB, challenger.parameters);
        let mut teacher_player = InProcessPlayer::new(teacher_network, Arc::clone(&self.tablebase), PAIR_HASH_MB, teacher.parameters);
        let recording =
            Recording { event_name: "temper-chess arena", round: 1, champion_name: &teacher.network, challenger_name: &challenger.network, pgn_path: "" };
        // A tune's games end early by the tablebase, win and draw rules; a round robin's and a gate's never.
        let early_stop = task.adjudicate.then_some(&*self.tablebase);
        let finished = runner::play_games(
            PREFIX, worker, &recording, control, 0, &[0, 1], fen, &mut challenger_player, &mut teacher_player, early_stop, &self.stop,
            &mut |_| {},
        );
        if finished.len() < 2 {
            return Ok(Played::Stopped);
        }
        Ok(Played::Pgn(finished.iter().map(|game| with_tags(&runner::pgn_text(&recording, game), &tags)).collect()))
    }

    /// Sends one task, retrying an unreachable smithy while its claim lasts, and applies the answer to
    /// the chore it was played for. Its claim gone, the task goes to the outbox (`keep`).
    fn send(&self, task: &Task, pgn: &str) {
        let body = task_body(&task.session, pgn);
        let (status, reply) = loop {
            let host = match self.claimed() {
                Some(claim) if claim.boot == task.boot => claim.host,
                _ => {
                    self.keep(task, body);
                    return;
                }
            };
            match self.post(&host, TASK_PATH, Some(&task.token), &body) {
                Ok(answer) => break answer,
                Err(message) => {
                    if self.stop.load(Ordering::SeqCst) {
                        return;
                    }
                    let mut dealing = self.dealing();
                    if !dealing.unreachable_said {
                        log::warn!("sending tasks: {message}; every worker trying again every {} s", RETRY.as_secs());
                        dealing.unreachable_said = true;
                    }
                    drop(dealing);
                    std::thread::sleep(RETRY);
                }
            }
        };
        let mut dealing = self.dealing();
        if dealing.unreachable_said {
            log::info!("the smithy answers tasks again");
            dealing.unreachable_said = false;
        }
        // An answer for a session since replaced says nothing of the current one.
        if dealing.joined.as_ref().map(|(session, _, _)| session) != Some(&task.session) {
            return;
        }
        let text = String::from_utf8_lossy(&reply).to_string();
        match status.as_str() {
            "OK" => {}
            "ADD" => match parse_entries(&text) {
                Ok(entries) => {
                    if let Some(chore) = dealing.chore.as_mut().filter(|chore| chore.id == task.chore) {
                        add_entries(chore, entries);
                    }
                }
                Err(message) => log::warn!("the smithy added entries this build cannot read: {message}"),
            },
            "STOP" => {
                if let Some(chore) = dealing.chore.as_mut().filter(|chore| chore.id == task.chore) {
                    chore.entries.iter_mut().for_each(|entry| entry.count = 0);
                }
            }
            "CHORE" => match Chore::parse(&text).and_then(|chore| check(&chore).map(|_| chore)) {
                Ok(chore) => dealing.set_chore(Some(chore)),
                Err(message) => log::warn!("the smithy sent a chore this build cannot play: {message}"),
            },
            "TOKEN" | "UNKNOWN" => dealing.forget(),
            other => log::warn!("task {} of chore {} not taken: {other} {}", task.seq, task.chore, text.trim()),
        }
        dealing.sent += 1;
    }

    /// This machine's speed with the network `name` names, benched the first time a tune names it.
    /// Held while benching, so the other workers wait and the bench runs on a quiet machine.
    fn speed(&self, name: &str, network: Option<&Network>) -> f64 {
        let mut speeds = self.speeds.lock().expect("the lock should not be poisoned");
        if let Some(&nps) = speeds.get(name) {
            return nps;
        }
        let threads = (self.hello.workers / 2).max(1);
        log::info!("benching {name} on {threads} threads, to scale the tune's clock to this machine");
        let nps = loaded_bench(network, threads);
        log::info!("bench: {nps:.0} nodes per second per thread");
        speeds.insert(name.to_string(), nps);
        nps
    }

    /// The network `name` names: `None` for the PeSTO evaluation, else its file in `store`, waited
    /// for while it is not there yet (`wait_for`), said once, then loaded and kept while the chore
    /// names it. A name is one exact file, never rewritten, so a network once loaded stays right.
    fn network(&self, store: &str, name: &str) -> Result<Fetched, String> {
        if name == PESTO {
            return Ok(Fetched::Ready(None));
        }
        if let Some(network) = self.networks.lock().expect("the lock should not be poisoned").get(name) {
            return Ok(Fetched::Ready(Some(Arc::clone(network))));
        }
        let path = store_path(store, name)?;
        // Waited for without the lock, so the workers playing networks already here go on.
        if !std::path::Path::new(&path).is_file() {
            if self.waiting_for.lock().expect("the lock should not be poisoned").insert(name.to_string()) {
                log::info!("waiting for {name} in {store}, up to {} minutes", NETWORK_TIMEOUT.as_secs() / 60);
            }
            match wait_for(&path, NETWORK_TIMEOUT, NETWORK_POLL, &self.stop) {
                Waited::Ready => {}
                Waited::Stopped => return Ok(Fetched::Stopped),
                Waited::TimedOut => return Ok(Fetched::Missing),
            }
        }
        // Held while loading, so two workers never read one network twice.
        let mut networks = self.networks.lock().expect("the lock should not be poisoned");
        if let Some(network) = networks.get(name) {
            return Ok(Fetched::Ready(Some(Arc::clone(network))));
        }
        let network = Arc::new(Network::load(&path).map_err(|error| format!("failed to load network {path}: {error}"))?);
        // The networks of earlier chores go: a run's candidates are many, and each is a few MB.
        let named: Vec<String> =
            self.dealing().chore.iter().flat_map(|chore| chore.entries.iter().flat_map(|entry| entry.players.iter().map(|player| player.network.clone()))).collect();
        networks.retain(|held, _| named.contains(held));
        networks.insert(name.to_string(), Arc::clone(&network));
        Ok(Fetched::Ready(Some(network)))
    }
}

/// What a worker's task came to.
enum Played {
    /// Its PGN, to send.
    Pgn(String),
    /// Stopped first: nothing to send.
    Stopped,
    /// The network named never reached the store: its chore is given back.
    Unavailable(String),
}

/// What looking for a chore's network came to.
enum Fetched {
    /// Loaded, `None` for the PeSTO evaluation.
    Ready(Option<Arc<Network>>),
    Stopped,
    /// Not in the store after `NETWORK_TIMEOUT`.
    Missing,
}

/// Network `name`'s file in `store`: `name` is its path below the store, `data/...`. A name of any
/// other shape is the smithy's error, refused rather than looked for.
pub(crate) fn store_path(store: &str, name: &str) -> Result<String, String> {
    if !name.starts_with("data/") || name.contains("..") {
        return Err(format!("the smithy named a network {name:?}, which is no path in the store"));
    }
    Ok(format!("{store}/{name}"))
}

/// What waiting for a file came to.
#[derive(Debug, PartialEq)]
pub(crate) enum Waited {
    Ready,
    TimedOut,
    Stopped,
}

/// Waits for `path` to be a file, looking every `poll`, until `timeout` or `stop`.
pub(crate) fn wait_for(path: &str, timeout: Duration, poll: Duration, stop: &AtomicBool) -> Waited {
    let started = Instant::now();
    loop {
        if std::path::Path::new(path).is_file() {
            return Waited::Ready;
        }
        if stop.load(Ordering::SeqCst) {
            return Waited::Stopped;
        }
        let waited = started.elapsed();
        if waited >= timeout {
            return Waited::TimedOut;
        }
        std::thread::sleep(poll.min(timeout - waited));
    }
}

/// The nodes per second of one search thread on this machine with `network`, loaded: `temper-chess bench` on
/// `threads` threads at once, as the smithy benches its own (its `loaded_bench`), the mean of
/// the threads.
fn loaded_bench(network: Option<&Network>, threads: usize) -> f64 {
    let runs: Vec<(u64, f64)> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..threads).map(|_| scope.spawn(move || temper_chess::bench::bench(network))).collect();
        handles.into_iter().map(|handle| handle.join().expect("a bench thread panicked")).collect()
    });
    runs.iter().map(|&(nodes, seconds)| nodes as f64 / seconds.max(1e-9)).sum::<f64>() / runs.len().max(1) as f64
}

/// Whether this build can play `chore`: a self-play game is played to a depth, with the parameters
/// its player gives (the smithy's forge gives the defaults), else the network's own.
fn check(chore: &Chore) -> Result<(), String> {
    if chore.kind == TaskKind::SelfPlay && !matches!(chore.limit, Limit::Depth(_)) {
        return Err(format!("chore {}: self-play under a clock", chore.id));
    }
    Ok(())
}

/// The index of the entry `chore`'s next task comes from: a tournament's with the most tasks left, the
/// first on a tie, so every matchup fills together; any other chore's first with tasks left, in their
/// order.
fn next_entry(chore: &Chore) -> Option<usize> {
    let mut left = chore.entries.iter().enumerate().filter(|(_, entry)| entry.count > 0);
    let found = match chore.kind {
        // `max_by_key` keeps the last of equals: reversed, the first.
        TaskKind::Tournament => left.rev().max_by_key(|(_, entry)| entry.count),
        _ => left.next(),
    };
    found.map(|(index, _)| index)
}

/// A top-up's `entries` into `chore`: a tournament's raise the quota of the entry of the same players
/// and tags, so it never holds more entries than matchups; any other chore's go after its own.
fn add_entries(chore: &mut Chore, entries: Vec<Entry>) {
    for entry in entries {
        let same = chore.entries.iter_mut().find(|held| held.players == entry.players && held.tags == entry.tags);
        match (chore.kind, same) {
            (TaskKind::Tournament, Some(held)) => held.count += entry.count,
            _ => chore.entries.push(entry),
        }
    }
}

/// The tags that identify `task`, then its entry's own.
fn identity(task: &Task) -> Vec<(String, String)> {
    let mut tags = vec![
        (CHORE_TAG.to_string(), task.chore.clone()),
        (SESSION_TAG.to_string(), task.session.clone()),
        (SEQ_TAG.to_string(), task.seq.to_string()),
    ];
    tags.extend(task.tags.iter().cloned());
    tags
}

/// `pgn`, one game, with `tags` added at the end of its tag section.
fn with_tags(pgn: &str, tags: &[(String, String)]) -> String {
    let end = pgn.find("\n\n").map_or(pgn.len(), |at| at + 1);
    let mut text = pgn[..end].to_string();
    for (name, value) in tags {
        text.push_str(&format!("[{name} \"{value}\"]\n"));
    }
    text.push_str(&pgn[end..]);
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_go_at_the_end_of_the_tag_section() {
        let pgn = "[Event \"e\"]\n[Result \"1-0\"]\n\n1. e4 1-0\n\n";
        let tags = vec![("Chore".to_string(), "c000001".to_string()), ("Seq".to_string(), "7".to_string())];
        assert_eq!(with_tags(pgn, &tags), "[Event \"e\"]\n[Result \"1-0\"]\n[Chore \"c000001\"]\n[Seq \"7\"]\n\n1. e4 1-0\n\n");
    }

    fn claim(smithy: &str, boot: &str) -> Claim {
        Claim { smithy: smithy.to_string(), host: smithy.to_string(), boot: boot.to_string(), commit: "c".to_string(), scaled: false, rank: None }
    }

    #[test]
    fn a_claim_that_outranks_the_held_one_takes_the_arena_afresh() {
        let now = Instant::now();
        let mut state = ClaimState::default();
        let scaled = Claim { scaled: true, rank: Some(0), ..claim("beta", "b1") };
        assert_eq!(state.take(scaled.clone(), now), Ok(true));
        state.outbox.push(("t".to_string(), b"task".to_vec()));
        let lower = Claim { rank: Some(1), ..scaled.clone() };
        assert_eq!(
            state.take(Claim { smithy: "alpha".to_string(), boot: "a1".to_string(), ..lower }, now),
            Err("the smithy ranked 1, for a scaled job".to_string())
        );
        let full = Claim { rank: Some(5), ..claim("alpha", "a1") };
        assert_eq!(state.take(full.clone(), now), Ok(true), "a full job's claim takes a scaled one's arena");
        assert!(state.outbox.is_empty(), "the scaled job's tasks dropped");
        assert_eq!(state.lapse(now), Ok(Some(full)));
        let higher = Claim { rank: Some(0), ..claim("gamma", "g1") };
        assert_eq!(state.take(higher, now), Ok(true), "between full jobs, the higher rank");
    }

    #[test]
    fn a_claim_is_turned_away_while_held_taken_over_by_a_restarted_smithy_and_lapses_unpolled() {
        let now = Instant::now();
        let mut state = ClaimState::default();
        assert_eq!(state.take(claim("beta", "b1"), now), Ok(true), "the first claim starts afresh");
        state.outbox.push(("t".to_string(), b"task".to_vec()));
        assert_eq!(state.take(claim("alpha", "b9"), now), Err("an unranked smithy, for a full job".to_string()), "another smithy's claim turned away");
        assert_eq!(state.take(claim("beta", "b1"), now), Ok(false), "the same boot again carries on");
        assert_eq!(state.outbox.len(), 1, "its outbox kept");

        let later = now + RELEASE_AFTER + Duration::from_secs(1);
        assert_eq!(state.lapse(now), Ok(Some(claim("beta", "b1"))), "polled, it holds");
        assert_eq!(state.lapse(later), Err(claim("beta", "b1")), "unpolled, it lapses");
        assert_eq!(state.lapse(later), Ok(None));
        assert_eq!(state.take(claim("beta", "b1"), later), Ok(false), "the boot last served, after a lapse, carries on");
        assert_eq!(state.outbox.len(), 1, "its outbox still kept");

        assert_eq!(state.take(claim("beta", "b2"), later), Ok(true), "a restarted smithy takes the arena over at once");
        assert!(state.outbox.is_empty(), "the earlier boot's tasks mean nothing to it");
        let silent = later + RELEASE_AFTER + Duration::from_secs(1);
        assert_eq!(state.take(claim("alpha", "b9"), silent), Ok(true), "a silent claim gives way to another smithy");
    }

    #[test]
    fn a_network_is_waited_for_until_it_is_there_the_timeout_or_a_stop() {
        let dir = std::env::temp_dir().join(format!("arena-wait-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("candidate.nnue").display().to_string();
        let (short, poll) = (Duration::from_millis(50), Duration::from_millis(10));
        let stop = AtomicBool::new(false);
        assert_eq!(wait_for(&path, short, poll, &stop), Waited::TimedOut, "never there");
        std::fs::write(&path, b"x").unwrap();
        assert_eq!(wait_for(&path, short, poll, &stop), Waited::Ready);
        std::fs::remove_file(&path).unwrap();
        stop.store(true, Ordering::SeqCst);
        assert_eq!(wait_for(&path, Duration::from_secs(60), poll, &stop), Waited::Stopped, "a stop does not wait out the timeout");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_network_s_name_is_its_path_in_the_store() {
        let name = "data/chess/networks/pesto-psq/run-1/gen-1/lambda-0.95/candidate.nnue";
        assert_eq!(store_path("G:/My Drive/kalo-games", name).unwrap(), format!("G:/My Drive/kalo-games/{name}"));
        for wrong in ["0123abcd", "/etc/x", "data/../x.nnue"] {
            assert!(store_path("G:/My Drive/kalo-games", wrong).is_err(), "{wrong}");
        }
    }

    #[test]
    fn self_play_is_played_to_a_depth_with_any_parameters() {
        let chore = |limit: Limit, parameters| Chore {
            id: "c1".to_string(),
            kind: TaskKind::SelfPlay,
            limit,
            adjudicate: false,
            reference_nps: None,
            entries: vec![protocol::Entry { players: vec![Player { network: PESTO.to_string(), parameters }], count: 1, tags: Vec::new() }],
        };
        assert!(check(&chore(Limit::Depth(9), None)).is_ok());
        assert!(check(&chore(Limit::Clock("10+0.1".to_string()), None)).is_err());
        assert!(check(&chore(Limit::Depth(9), Some(temper_chess::search::SearchParameters::DEFAULT))).is_ok());
    }

    fn matchup(first: &str, second: &str, count: u32) -> Entry {
        let own = |network: &str| Player { network: network.to_string(), parameters: None };
        Entry { players: vec![own(first), own(second)], count, tags: Vec::new() }
    }

    fn chore_of(kind: TaskKind, entries: Vec<Entry>) -> Chore {
        Chore { id: "c1".to_string(), kind, limit: Limit::Depth(9), adjudicate: false, reference_nps: None, entries }
    }

    /// The second player of each task `chore` deals until it has none left.
    fn order(chore: &mut Chore) -> Vec<String> {
        let mut seconds = Vec::new();
        while let Some(index) = next_entry(chore) {
            chore.entries[index].count -= 1;
            seconds.push(chore.entries[index].players[1].network.clone());
        }
        seconds
    }

    #[test]
    fn a_tournament_plays_its_matchups_together() {
        let mut chore = chore_of(TaskKind::Tournament, vec![matchup("aa", "bb", 2), matchup("aa", "cc", 3), matchup("bb", "cc", 2)]);
        assert_eq!(order(&mut chore), ["cc", "bb", "cc", "cc", "bb", "cc", "cc"]);
        let mut pair = chore_of(TaskKind::Pair, vec![matchup("aa", "bb", 2), matchup("aa", "cc", 1)]);
        assert_eq!(order(&mut pair), ["bb", "bb", "cc"], "any other chore in its entries' order");
    }

    #[test]
    fn a_tournament_s_top_up_raises_its_matchups() {
        let mut chore = chore_of(TaskKind::Tournament, vec![matchup("aa", "bb", 1), matchup("aa", "cc", 0)]);
        add_entries(&mut chore, vec![matchup("aa", "cc", 4), matchup("bb", "cc", 2)]);
        assert_eq!(chore.entries.iter().map(|entry| entry.count).collect::<Vec<_>>(), [1, 4, 2], "one entry a matchup");
        let mut pair = chore_of(TaskKind::Pair, vec![matchup("aa", "bb", 1)]);
        add_entries(&mut pair, vec![matchup("aa", "bb", 2)]);
        assert_eq!(pair.entries.len(), 2, "any other chore's after its own");
    }
}
