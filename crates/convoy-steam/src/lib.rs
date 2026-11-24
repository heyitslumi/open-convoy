pub mod gameserver;
pub mod sockets;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum SteamError {
    #[error("Library load error: {0}")]
    LibraryLoadError(String),
    #[error("Init failed")]
    InitFailed,
    #[error("Auth failed with code: {0}")]
    AuthFailed(i32),
    #[error("Socket error: {0}")]
    SocketError(String),
}
