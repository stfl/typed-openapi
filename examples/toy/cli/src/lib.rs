//! The toy accounting CLI, as a library so its tests drive the real thing.
//!
//! One module per job: [`app`] is the command tree and the dispatch, [`raw`]
//! and [`finalize`] are the two verbs it dispatches to, [`output`] is what they
//! all hand back, and [`client`] is where a request meets the network.
//!
//! `main.rs` is a shell around [`app::run`]: it picks the HTTP client, hands
//! over the parsed arguments, and prints what comes back. Everything a test
//! would want to assert on is in [`output::Output`].

pub mod app;
pub mod client;
pub mod finalize;
pub mod output;
pub mod raw;
