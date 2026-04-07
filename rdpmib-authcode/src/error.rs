use std::{io, sync::mpsc::RecvError};

use tao::error::OsError;
use thiserror::Error;
use url::ParseError;

#[derive(Debug, Error)]
pub enum AuthCodeError {
    #[error("{0}")]
    Io(#[from] io::Error),

    #[error("{0}")]
    Os(#[from] OsError),

    #[error("{0}")]
    Wry(#[from] wry::Error),

    #[error("busy")]
    Busy,

    #[error("{0}")]
    UrlParse(#[from] ParseError),

    #[error("{0}")]
    Failed(String),

    #[error("event loop closed.")]
    EventLoopClosed,

    #[error("{0}")]
    Recv(#[from] RecvError),
}
