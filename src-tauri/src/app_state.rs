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
    model_credentials: Option<Arc<shixu_native::model_client::credential::ModelCredentialStore>>,
    model_transport: Option<Arc<shixu_native::model_client::http::DeepSeekTransport>>,
    model_error: std::sync::Mutex<Option<AppError>>,
    vault: Arc<crate::vault::VaultController>,
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
            model_credentials: None,
            model_transport: None,
            model_error: std::sync::Mutex::new(None),
            vault: crate::vault::VaultController::unavailable(),
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
    pub fn with_model(mut self, root: &std::path::Path) -> AppResult<Self> {
        let credentials = Arc::new(
            shixu_native::model_client::credential::ModelCredentialStore::new(root.into())?,
        );
        self.model_transport = Some(Arc::new(
            shixu_native::model_client::http::DeepSeekTransport::new(credentials.clone())?,
        ));
        self.model_credentials = Some(credentials);
        Ok(self)
    }
    pub fn model_credentials(
        &self,
    ) -> AppResult<Arc<shixu_native::model_client::credential::ModelCredentialStore>> {
        self.model_credentials.clone().ok_or(AppError::Unsupported)
    }
    pub fn model_transport(
        &self,
    ) -> AppResult<Arc<shixu_native::model_client::http::DeepSeekTransport>> {
        self.model_transport.clone().ok_or(AppError::Unsupported)
    }
    pub fn model_error(&self) -> AppResult<Option<AppError>> {
        Ok(*self.model_error.lock().map_err(|_| AppError::Conflict)?)
    }
    pub fn test_model(&self) -> AppResult<()> {
        let result = self.model_transport()?.test_connection();
        *self.model_error.lock().map_err(|_| AppError::Conflict)? = result.as_ref().err().copied();
        result
    }
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
            vault: crate::vault::VaultController::unavailable(),
            qq: Some(qq),
            model_credentials: None,
            model_transport: None,
            model_error: std::sync::Mutex::new(None),
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
    pub fn with_vault(mut self, resources: std::path::PathBuf, work: std::path::PathBuf) -> Self {
        self.vault = crate::vault::VaultController::prepared(resources, work);
        self
    }
    pub fn vault(&self) -> Arc<crate::vault::VaultController> {
        self.vault.clone()
    }
    #[cfg(windows)]
    pub fn with_fictional_vault(
        mut self,
        resources: std::path::PathBuf,
        work: std::path::PathBuf,
    ) -> Self {
        self.vault = crate::vault::VaultController::fictional(resources, work);
        self
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
