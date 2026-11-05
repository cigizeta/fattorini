//! Ctrl+C handling for `arena`'s matches and the gate: a `ctrlc::set_handler` setting an
//! `Arc<AtomicBool>`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// The matches' and the gate's handler: one press asks the run to stop, and the run drops the game
/// in flight rather than waiting for it.
pub fn install_ctrlc_handler() -> Arc<AtomicBool> {
    let stop_requested = Arc::new(AtomicBool::new(false));
    let stop_requested_handler = Arc::clone(&stop_requested);
    ctrlc::set_handler(move || {
        stop_requested_handler.store(true, Ordering::SeqCst);
    })
    .expect("failed to install the Ctrl+C handler");
    stop_requested
}
