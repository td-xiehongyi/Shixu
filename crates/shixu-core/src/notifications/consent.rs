//! Model authorization is separate from source collection and attachment parsing.
use crate::contracts::{AppResult, error::AppError, notification::MessageEnvelope};
use crate::storage::coordinator::WriteCoordinator;
use std::sync::{Arc, Mutex};
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
    pub(crate) instance_id: String,
    coordinator: Mutex<Option<Arc<WriteCoordinator>>>,
    pub(crate) current: Mutex<ModelConsent>,
}
impl ConsentStore {
    pub fn new(consent: ModelConsent) -> Self {
        Self {
            instance_id: uuid::Uuid::new_v4().to_string(),
            coordinator: Mutex::new(None),
            current: Mutex::new(consent),
        }
    }
    /// Bind once to the same authority used by database settings and runtime.
    pub fn bind(&self, coordinator: Arc<WriteCoordinator>) -> AppResult<()> {
        let mut bound = self.coordinator.lock().map_err(|_| AppError::Conflict)?;
        if let Some(existing) = bound.as_ref() {
            if !Arc::ptr_eq(existing, &coordinator) {
                return Err(AppError::Conflict);
            }
        } else {
            *bound = Some(coordinator);
        }
        Ok(())
    }
    pub fn snapshot(&self) -> AppResult<ModelConsent> {
        Ok(self.current.lock().map_err(|_| AppError::Conflict)?.clone())
    }
    pub fn replace(&self, expected: u64, mut consent: ModelConsent) -> AppResult<()> {
        let coordinator = self
            .coordinator
            .lock()
            .map_err(|_| AppError::Conflict)?
            .clone();
        let _permit = coordinator.as_ref().map(|c| c.enter()).transpose()?;
        let mut current = self.current.lock().map_err(|_| AppError::Conflict)?;
        if current.revision != expected {
            return Err(AppError::Conflict);
        }
        consent.revision = expected.checked_add(1).ok_or(AppError::Conflict)?;
        *current = consent;
        Ok(())
    }
}
