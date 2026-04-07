use std::sync::mpsc;

use super::AuthCodeError;

#[derive(Debug)]
pub(super) enum UserEvent {
    StateChanged,
    PageLoading(String),

    RequestGetCode(
        String,
        mpsc::Sender<std::result::Result<String, AuthCodeError>>,
    ),
}
