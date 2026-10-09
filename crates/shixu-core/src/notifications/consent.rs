//! Model authorization is separate from source collection and attachment parsing.
use crate::contracts::{AppResult, error::AppError, notification::MessageEnvelope};
use std::sync::Mutex;
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelConsent {
    pub enabled: bool,
    pub provider_id: Option<String>,
    pub allowed_group_ids: Vec<String>,
    pub allow_attachment_text: bool,
    pub revision: u64,
}
pub fn authorize_request(
    consent: &ModelConsent,
    message: &MessageEnvelope,
    attachment: bool,
) -> AppResult<()> {
    if !consent.enabled
        || message.revoked
        || !consent.allowed_group_ids.contains(&message.group_id)
        || consent
            .provider_id
            .as_ref()
            .is_none_or(|p| p.is_empty() || p.len() > 128 || p.chars().any(char::is_control))
        || attachment && !consent.allow_attachment_text
    {
        return Err(AppError::InvalidInput);
    }
    Ok(())
}
/// Every sender and settings writer must share this store. The dispatch lock
/// linearizes consent changes and actual transport admission. It is held through
/// the bounded transport call; callers must not re-enter this store from send.
pub struct ConsentStore {
    pub(crate) current: Mutex<ModelConsent>,
}
impl ConsentStore {
    pub fn new(consent: ModelConsent) -> Self {
        Self {
            current: Mutex::new(consent),
        }
    }
    pub fn snapshot(&self) -> AppResult<ModelConsent> {
        Ok(self.current.lock().map_err(|_| AppError::Conflict)?.clone())
    }
    pub fn replace(&self, expected: u64, mut consent: ModelConsent) -> AppResult<()> {
        let mut current = self.current.lock().map_err(|_| AppError::Conflict)?;
        if current.revision != expected {
            return Err(AppError::Conflict);
        }
        consent.revision = expected.checked_add(1).ok_or(AppError::Conflict)?;
        *current = consent;
        Ok(())
    }
}
