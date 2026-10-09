use shixu_core::contracts::{error::AppError, vault::SecretBytes};
use shixu_native::model_client::protect_api_token;
#[test]
fn credentials_reject_invalid_provider_and_empty_tokens() {
    assert_eq!(
        protect_api_token("", SecretBytes::new(vec![1])).err(),
        Some(AppError::InvalidInput)
    );
    assert_eq!(
        protect_api_token("provider", SecretBytes::new(vec![])).err(),
        Some(AppError::InvalidInput)
    );
}
#[cfg(not(windows))]
#[test]
fn credential_os_protection_has_no_plaintext_fallback() {
    assert_eq!(
        protect_api_token("synthetic", SecretBytes::new(vec![1, 2, 3])).err(),
        Some(AppError::Unsupported)
    );
}
#[test]
fn production_request_candidates_is_explicitly_unsupported() {
    use shixu_core::{contracts::notification::*, notifications::consent::ModelConsent};
    let id = uuid::Uuid::nil();
    let message = MessageEnvelope {
        message_key: MessageKey::from_uuid(id),
        source_id: SourceId::from_uuid(id),
        account_id: "synthetic".into(),
        group_id: "g1".into(),
        native_message_id: "test".into(),
        sent_at: 1791504000000,
        received_at: 1791504000000,
        sender_id: "synthetic".into(),
        text: "2026年10月12日高数测验".into(),
        reply_to: None,
        revision: 1,
        revoked: false,
        processing_state: ProcessingState::Persisted,
        parts: vec![],
    };
    let consent = ModelConsent {
        enabled: true,
        provider_id: Some("synthetic".into()),
        allowed_group_ids: vec!["g1".into()],
        allow_attachment_text: true,
        revision: 1,
    };
    assert_eq!(
        shixu_native::model_client::request_candidates(&consent, &message, &[]),
        Err(AppError::Unsupported)
    );
}
