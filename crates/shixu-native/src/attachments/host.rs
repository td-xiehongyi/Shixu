use serde::{Deserialize, Serialize};
use shixu_core::contracts::{AppResult, error::AppError, notification::*};
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DetectedType {
    Png,
    Jpeg,
    Pdf,
    Docx,
    Xlsx,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParseRequest {
    pub part_id: PartId,
    pub input_handle: u64,
    pub detected_type: DetectedType,
    pub limits: ParserLimits,
}
/// No parser process or in-process fallback until the native isolation gate
/// passes. This deliberately remains Unsupported on Windows as well as Linux.
pub fn parse_part(_part: &MessagePart, _limits: &ParserLimits) -> AppResult<PartResult> {
    Err(AppError::Unsupported)
}
