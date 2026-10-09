//! No provider is configured. The production entry point deliberately never sends.
use crate::protection::DpapiProtector;
use shixu_core::{
    contracts::{AppResult, error::AppError, notification::*, vault::SecretBytes},
    notifications::consent::ModelConsent,
    storage::DataProtector,
};
pub fn request_candidates(
    _: &ModelConsent,
    _: &MessageEnvelope,
    _: &[EvidenceBlock],
) -> AppResult<Vec<Candidate>> {
    Err(AppError::Unsupported)
}
/// Independent model API credential, OS-protected and purpose/provider-bound.
/// No QQ credential, vault session, master password or serializable secret DTO.
/// Persistence and actual provider configuration remain native integration gates.
/// ```compile_fail
/// use shixu_native::model_client::ProtectedModelCredential;
/// fn log(value: &ProtectedModelCredential) { println!("{value:?}"); }
/// ```
pub struct ProtectedModelCredential {
    sealed: Vec<u8>,
}
fn prefix(provider: &str) -> AppResult<Vec<u8>> {
    if provider.is_empty() || provider.len() > 128 || provider.chars().any(char::is_control) {
        return Err(AppError::InvalidInput);
    }
    let mut prefix = b"shixu-model-api-token-v1\0".to_vec();
    prefix.extend_from_slice(provider.as_bytes());
    prefix.push(0);
    Ok(prefix)
}
pub fn protect_api_token(
    provider: &str,
    token: SecretBytes,
) -> AppResult<ProtectedModelCredential> {
    let mut plain = prefix(provider)?;
    if token.expose().is_empty() || token.expose().len() > 8192 {
        return Err(AppError::InvalidInput);
    }
    plain.extend_from_slice(token.expose());
    let plain = SecretBytes::new(plain);
    Ok(ProtectedModelCredential {
        sealed: DpapiProtector.protect(plain.expose())?,
    })
}
pub fn unprotect_api_token(
    provider: &str,
    credential: &ProtectedModelCredential,
) -> AppResult<SecretBytes> {
    let prefix = prefix(provider)?;
    let plain = SecretBytes::new(DpapiProtector.unprotect(&credential.sealed)?);
    let token = plain
        .expose()
        .strip_prefix(prefix.as_slice())
        .ok_or(AppError::AuthFailed)?;
    if token.is_empty() || token.len() > 8192 {
        return Err(AppError::AuthFailed);
    }
    Ok(SecretBytes::new(token.to_vec()))
}
