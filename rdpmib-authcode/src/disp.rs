use std::sync::mpsc;

use tao::event_loop::EventLoopProxy;

use super::AuthCodeError;
use super::event::UserEvent;

type Result<T> = std::result::Result<T, AuthCodeError>;

#[derive(Debug)]
pub struct Dispatcher {
    proxy: EventLoopProxy<UserEvent>,
}

impl Dispatcher {
    pub(super) fn new(proxy: EventLoopProxy<UserEvent>) -> Self {
        Self { proxy }
    }

    pub fn request_get_code(&self, url: &str) -> Result<String> {
        let (tx, rx) = mpsc::channel();
        self.proxy
            .send_event(UserEvent::RequestGetCode(url.to_string(), tx))
            .or(Err(AuthCodeError::EventLoopClosed))?;
        rx.recv()?
    }
}
