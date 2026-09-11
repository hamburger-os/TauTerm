//! Protocol-agnostic transport runtime.
//!
//! The transport layer owns physical byte/datagram resources. Protocol/session code consumes
//! DataPlane handles and never depends on the concrete driver or its blocking model.

pub mod error;
pub mod runtime;
pub mod serial;
pub mod stream;
pub mod tcp;
pub mod udp;

pub use error::{TransportError, TransportErrorKind};
pub use runtime::{
    DataPlaneEvent, DataPlaneHandle, DataPlaneRuntime, ExclusiveIo, TransportCloseInfo,
};
pub use stream::{BlockingByteStream, ReadStatus};
