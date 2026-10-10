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
    qq: Option<Arc<shixu_native::qq::connection::ConnectionController>>,
    qq_receiver: std::sync::Mutex<Option<Box<dyn shixu_core::runtime::workers::ReceivePort>>>,
    backup: Option<Arc<shixu_core::backup::CalendarBackup>>,
    runtime: Option<Arc<shixu_core::runtime::Supervisor>>,
    calendar: Option<EventService>,
    db: Option<Arc<Database>>,
    pub consent: Arc<ConsentStore>,
}
impl Default for AppState {
    fn default() -> Self {
        Self {
            qq: None,
            qq_receiver: std::sync::Mutex::new(None),
            backup: None,
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
        let (qq, receiver) = shixu_native::qq::connection::ConnectionController::new(db.clone());
        Self {
            qq: Some(qq),
            qq_receiver: std::sync::Mutex::new(Some(receiver)),
            backup: None,
            runtime: Some(Arc::new(shixu_core::runtime::Supervisor::new(
                db.clone(),
                consent.clone(),
            ))),
            calendar: Some(EventService::new(db.clone())),
            db: Some(db),
            consent,
        }
    }
    pub fn with_backups(mut self, root: &std::path::Path) -> AppResult<Self> {
        self.backup = Some(Arc::new(shixu_core::backup::CalendarBackup::new(
            self.database()?,
            self.consent.clone(),
            root,
        )?));
        Ok(self)
    }
    pub fn qq(&self) -> AppResult<Arc<shixu_native::qq::connection::ConnectionController>> {
        self.qq.clone().ok_or(AppError::Unsupported)
    }
    pub fn take_qq_receiver(
        &self,
    ) -> AppResult<Box<dyn shixu_core::runtime::workers::ReceivePort>> {
        self.qq_receiver
            .lock()
            .map_err(|_| AppError::Disconnected)?
            .take()
            .ok_or(AppError::Conflict)
    }
    pub fn backup(&self) -> AppResult<Arc<shixu_core::backup::CalendarBackup>> {
        self.backup.clone().ok_or(AppError::Unsupported)
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
