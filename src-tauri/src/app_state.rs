use shixu_core::{
    calendar::EventService,
    contracts::{AppResult, error::AppError},
    notifications::{
        MessageStore,
        consent::{ConsentStore, ModelConsent},
        settings::SettingsStore,
    },
    storage::Database,
};
use std::sync::Arc;
/// One database and live consent authority shared with the background runtime.
pub struct AppState {
    runtime: Option<Arc<shixu_core::runtime::Supervisor>>,
    calendar: Option<EventService>,
    db: Option<Arc<Database>>,
    pub consent: Arc<ConsentStore>,
}
impl Default for AppState {
    fn default() -> Self {
        Self {
            runtime: None,
            calendar: None,
            db: None,
            consent: Arc::new(ConsentStore::new(ModelConsent::default())),
        }
    }
}
impl AppState {
    pub fn new(calendar: EventService) -> Self {
        Self {
            calendar: Some(calendar),
            ..Self::default()
        }
    }
    pub fn from_database(db: Arc<Database>) -> Self {
        let consent = Arc::new(ConsentStore::new(ModelConsent::default()));
        consent
            .bind(db.coordinator())
            .expect("fresh consent binding");
        Self {
            runtime: Some(Arc::new(shixu_core::runtime::Supervisor::new(
                db.clone(),
                consent.clone(),
            ))),
            calendar: Some(EventService::new(db.clone())),
            db: Some(db),
            consent,
        }
    }
    pub fn runtime(&self) -> AppResult<Arc<shixu_core::runtime::Supervisor>> {
        self.runtime.clone().ok_or(AppError::Unsupported)
    }
    pub fn calendar(&self) -> AppResult<&EventService> {
        self.calendar.as_ref().ok_or(AppError::Unsupported)
    }
    pub fn messages(&self) -> AppResult<MessageStore> {
        Ok(MessageStore::new(self.database()?))
    }
    pub fn settings(&self) -> AppResult<SettingsStore> {
        Ok(SettingsStore::new(self.database()?))
    }
    pub fn database(&self) -> AppResult<Arc<Database>> {
        self.db.clone().ok_or(AppError::Unsupported)
    }
}
