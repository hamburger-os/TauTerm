//! Protocol-agnostic session runtime primitives.
//!
//! Session orchestration owns lifecycle, script-facing I/O and user-visible disconnect semantics.
//! Concrete transport drivers live under `crate::transport`; protocol-specific services remain in
//! their plugins.

pub mod error;
pub mod io;
pub mod state;

pub use error::SessionError;
pub use io::{SessionIo, SessionIoError, TargetedIo};
pub use state::{DisconnectInfo, DisconnectKind};
