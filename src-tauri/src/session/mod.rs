//! Protocol-agnostic session runtime primitives.
//!
//! Session orchestration owns lifecycle, script-facing I/O and user-visible disconnect semantics.
//! Concrete transport drivers live under `crate::transport`; protocol-specific services remain in
//! their plugins.

pub mod error;
pub mod io;
pub mod runtime;
pub mod state;

pub use error::SessionError;
pub use io::{AutomationIo, AutomationRx, AutomationRxEvent, SessionIo, SessionIoError};
pub use runtime::SessionDataPlane;
pub use state::DisconnectInfo;
mod identity;
pub use identity::{RuntimeId, RuntimeIdentity};

/// 交互式终端的初始几何（列、行）。
///
/// 几何的唯一所有者是前端：它按真实视口 `fit()` 后通过 `resize_pty` 下发。
/// 后端只在“PTY 已经建立、首个尺寸还没到”这段窗口里需要一个确定的初始值，
/// 该值必须显式给出，不能依赖 PTY 库的默认值——portable-pty 的默认是 24×80，
/// 而全屏 TUI（vim/less）会按启动瞬间的行列排版，窗口越不匹配、错位越明显。
/// 120×30 是桌面工作区的典型可用尺寸。
pub const DEFAULT_TERMINAL_COLUMNS: u16 = 120;
pub const DEFAULT_TERMINAL_ROWS: u16 = 30;
