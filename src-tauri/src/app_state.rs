use shixu_core::{
    calendar::EventService,
    contracts::{AppResult, error::AppError},
};
/// Calendar owns its independent protected database. No vault secrets/session in public state.
#[derive(Default)]
pub struct AppState {
    calendar: Option<EventService>,
}
impl AppState {
    pub fn new(calendar: EventService) -> Self {
        Self {
            calendar: Some(calendar),
        }
    }
    pub fn calendar(&self) -> AppResult<&EventService> {
        self.calendar.as_ref().ok_or(AppError::Unsupported)
    }
}
