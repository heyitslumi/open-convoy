//! Wire protocol types and framing for the OpenConvoy server.

pub mod codec;
pub mod frame;
pub mod messages;
pub mod rpc;

pub use frame::{parse_frame, FrameType, ProtoError, RawFrame};
