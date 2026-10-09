//! Portable command policy. Context is supplied by the native calling WebView, never payload fields.
use crate::{
    app_state::AppState,
    wire::{self, WirePatch, WireUndo},
};
use serde::Deserialize;
use shixu_core::contracts::{AppResult, calendar::EventQuery, error::AppError};
pub const COMMANDS: &[&str] = &[
    "calendar_create_manual",
    "calendar_details",
    "notification_list",
    "notification_parts",
    "retry_part",
    "settings_read",
    "save_source_config",
    "set_model_consent",
    "set_autostart",
    "calendar_query",
    "calendar_edit",
    "calendar_undo",
    "show_vault_window",
    "vault_create",
    "vault_change_master",
    "vault_copy",
    "vault_unlock",
    "vault_list",
    "vault_apply",
    "vault_reveal",
    "vault_lock",
];
pub struct CallingContext<'a> {
    pub label: &'a str,
    pub origin: &'a str,
}
pub fn authorize(context: &CallingContext<'_>, command: &str) -> AppResult<()> {
    // Product Windows origin only; no remote/dev HTTP origins get native access.
    if context.origin != "http://tauri.localhost" || !COMMANDS.contains(&command) {
        return Err(AppError::AuthFailed);
    }
    let permitted = match context.label {
        "main" => matches!(
            command,
            "calendar_query"
                | "calendar_edit"
                | "calendar_undo"
                | "show_vault_window"
                | "calendar_create_manual"
                | "calendar_details"
                | "notification_list"
                | "notification_parts"
                | "retry_part"
                | "settings_read"
                | "save_source_config"
                | "set_model_consent"
                | "set_autostart"
        ),
        "vault" => command.starts_with("vault_"),
        _ => false,
    };
    if permitted {
        Ok(())
    } else {
        Err(AppError::AuthFailed)
    }
}
pub fn validate_size(bytes: usize) -> AppResult<()> {
    if bytes > 65536 {
        Err(AppError::InvalidInput)
    } else {
        Ok(())
    }
}
pub fn validate_id(value: &str) -> AppResult<()> {
    if value.len() != 36 {
        return Err(AppError::InvalidInput);
    }
    let _: shixu_core::contracts::calendar::EventId = value.parse()?;
    Ok(())
}

fn decode<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> AppResult<T> {
    serde_json::from_value(value).map_err(|_| AppError::InvalidInput)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryArgs {
    query: EventQuery,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EditArgs {
    id: String,
    revision: String,
    patch: WirePatch,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UndoArgs {
    request: WireUndo,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyArgs {}
fn serialized<T: serde::Serialize>(value: T) -> AppResult<serde_json::Value> {
    serde_json::to_value(value).map_err(|_| AppError::Unsupported)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IdArgs {
    id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ManualArgs {
    patch: WirePatch,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListArgs {
    source_id: Option<shixu_core::contracts::notification::SourceId>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PartsArgs {
    message_key: shixu_core::contracts::notification::MessageKey,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RetryArgs {
    part_id: shixu_core::contracts::notification::PartId,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceArgs {
    config: shixu_core::contracts::notification::SourceConfig,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConsentArgs {
    consent: wire::WireConsent,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AutostartArgs {
    enabled: bool,
}
/// Full portable dispatch is also used by the Windows adapter. Errors are fixed codes.
pub fn dispatch(
    context: &CallingContext<'_>,
    command: &str,
    payload: serde_json::Value,
    state: &AppState,
) -> AppResult<serde_json::Value> {
    authorize(context, command)?;
    validate_json_size(&payload)?;
    // Do not serialize secret-bearing vault payloads into temporary strings.
    if command.starts_with("vault_") {
        validate_vault(command, payload)?;
        return Err(AppError::Unsupported);
    }

    match command {
        "calendar_create_manual" => {
            let a: ManualArgs = decode(payload)?;
            serialized(wire::event(
                state.calendar()?.create_manual(a.patch.into_core()?)?,
            )?)
        }
        "calendar_details" => {
            let a: IdArgs = decode(payload)?;
            validate_id(&a.id)?;
            let service = state.calendar()?;
            let history = service
                .history(&a.id)?
                .into_iter()
                .rev()
                .take(100)
                .map(|h| {
                    Ok(wire::WireChange {
                        change_id: h.change_id,
                        before: h.before.map(wire::event).transpose()?,
                        after: wire::event(h.after)?,
                        undone: h.undone,
                    })
                })
                .collect::<AppResult<Vec<_>>>()?;
            let sources = service
                .sources(&a.id)?
                .into_iter()
                .take(100)
                .map(|s| wire::WireSource {
                    message_key: s.message_key,
                    message_revision: s.message_revision.into(),
                    group_id: wire::excerpt(&s.group_id, 128),
                    outcome: s.outcome,
                    evidence: wire::blocks(s.candidate.evidence),
                })
                .collect();
            serialized(wire::WireDetails {
                origin: service.origin(&a.id)?,
                history,
                sources,
            })
        }
        "notification_list" => {
            let a: ListArgs = decode(payload)?;
            serialized(
                state
                    .messages()?
                    .list(a.source_id, 100)?
                    .into_iter()
                    .map(|m| {
                        let applied = state
                            .calendar()?
                            .message_applied(m.message_key, m.revision)?;
                        wire::message(m, applied)
                    })
                    .collect::<AppResult<Vec<_>>>()?,
            )
        }
        "notification_parts" => {
            let a: PartsArgs = decode(payload)?;
            let parts = state
                .messages()?
                .current_parts(a.message_key)?
                .into_iter()
                .take(5)
                .map(|mut p| {
                    p.blocks = wire::blocks(p.blocks);
                    p
                })
                .collect::<Vec<_>>();
            serialized(parts)
        }
        "retry_part" => {
            let a: RetryArgs = decode(payload)?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| AppError::Unsupported)?
                .as_millis();
            let now = i64::try_from(now).map_err(|_| AppError::InvalidInput)?;
            shixu_core::notifications::parts::TaskQueue::new(state.database()?)
                .retry_part(a.part_id, now)?;
            Ok(serde_json::Value::Null)
        }
        "settings_read" => {
            let _: EmptyArgs = decode(payload)?;
            let settings = state.settings()?;
            serialized(wire::WireSettings {
                sources: settings
                    .sources()?
                    .into_iter()
                    .map(|s| wire::WireSourceSetting {
                        config: s.config,
                        epoch: s.epoch.into(),
                    })
                    .collect(),
                model: state.consent.snapshot()?.into(),
                autostart: settings.autostart()?,
                transport_supported: false,
            })
        }
        "save_source_config" => {
            let a: SourceArgs = decode(payload)?;
            state.settings()?.save_source(a.config)?;
            Ok(serde_json::Value::Null)
        }
        "set_model_consent" => {
            let a: ConsentArgs = decode(payload)?;
            state.settings()?;
            let consent = a.consent.into_core()?;
            state.consent.replace(consent.revision, consent)?;
            Ok(serde_json::Value::Null)
        }
        "set_autostart" => {
            let a: AutostartArgs = decode(payload)?;
            state.settings()?.set_autostart(a.enabled)?;
            Ok(serde_json::Value::Null)
        }
        "calendar_query" => {
            let a: QueryArgs = decode(payload)?;
            if a.query.statuses.len() > 3 {
                return Err(AppError::InvalidInput);
            }
            for date in [&a.query.from_date, &a.query.through_date]
                .into_iter()
                .flatten()
            {
                wire::text(date, 10)?;
            }
            let result = state.calendar()?.query(a.query)?;
            serialized(
                result
                    .into_iter()
                    .map(wire::event)
                    .collect::<AppResult<Vec<_>>>()?,
            )
        }
        "calendar_edit" => {
            let a: EditArgs = decode(payload)?;
            validate_id(&a.id)?;
            let rev = wire::revision(&a.revision)?;
            let patch = a.patch.into_core()?;
            if patch.event_id.to_string() != a.id.to_ascii_lowercase()
                || patch.expected_revision != rev
            {
                return Err(AppError::InvalidInput);
            }
            serialized(wire::event(state.calendar()?.edit(&a.id, rev, patch)?)?)
        }
        "calendar_undo" => {
            let a: UndoArgs = decode(payload)?;
            serialized(wire::event(state.calendar()?.undo(a.request.into())?)?)
        }
        "show_vault_window" => {
            let _: EmptyArgs = decode(payload)?;
            Ok(serde_json::Value::Null)
        }
        _ => Err(AppError::AuthFailed),
    }
}
// Deserialize secret arrays into zeroizing ownership; no Debug, logs, sessions or real engine.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UnlockArgs {
    master: zeroize::Zeroizing<Vec<u8>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChangeMasterArgs {
    current: zeroize::Zeroizing<Vec<u8>>,
    next: zeroize::Zeroizing<Vec<u8>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CopyArgs {
    id: String,
    field: CopyField,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum CopyField {
    Account,
    Password,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RevealArgs {
    id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MutationArgs {
    mutation: VaultMutationWire,
}
#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
enum VaultMutationWire {
    Create {
        channel: String,
        account: String,
        password: zeroize::Zeroizing<Vec<u8>>,
    },
    Update {
        id: String,
        expected_revision: wire::Decimal,
        channel: String,
        account: String,
        password: zeroize::Zeroizing<Vec<u8>>,
    },
    Delete {
        id: String,
        expected_revision: wire::Decimal,
    },
}
fn secret(value: &[u8]) -> AppResult<()> {
    if value.is_empty() || value.len() > 65536 {
        Err(AppError::InvalidInput)
    } else {
        Ok(())
    }
}
fn validate_vault(command: &str, payload: serde_json::Value) -> AppResult<()> {
    match command {
        "vault_change_master" => {
            let a: ChangeMasterArgs = decode(payload)?;
            secret(&a.current)?;
            secret(&a.next)
        }
        "vault_copy" => {
            let a: CopyArgs = decode(payload)?;
            let _ = a.field;
            validate_id(&a.id)
        }
        "vault_unlock" | "vault_create" => {
            let a: UnlockArgs = decode(payload)?;
            secret(&a.master)
        }
        "vault_reveal" => {
            let a: RevealArgs = decode(payload)?;
            validate_id(&a.id)
        }
        "vault_apply" => {
            let a: MutationArgs = decode(payload)?;
            match a.mutation {
                VaultMutationWire::Create {
                    channel,
                    account,
                    password,
                } => {
                    wire::text(&channel, 4096)?;
                    wire::text(&account, 4096)?;
                    secret(&password)
                }
                VaultMutationWire::Update {
                    id,
                    expected_revision,
                    channel,
                    account,
                    password,
                } => {
                    validate_id(&id)?;
                    let _: String = expected_revision.into();
                    wire::text(&channel, 4096)?;
                    wire::text(&account, 4096)?;
                    secret(&password)
                }
                VaultMutationWire::Delete {
                    id,
                    expected_revision,
                } => {
                    validate_id(&id)?;
                    let _: String = expected_revision.into();
                    Ok(())
                }
            }
        }
        "vault_list" | "vault_lock" => {
            let _: EmptyArgs = decode(payload)?;
            Ok(())
        }
        _ => Err(AppError::AuthFailed),
    }
}

/// Bound parsed IPC objects without creating another secret-bearing JSON buffer.
/// Conservative escaped-string accounting; nesting/collection work is bounded too.
pub fn validate_json_size(value: &serde_json::Value) -> AppResult<()> {
    fn count(value: &serde_json::Value, depth: usize, budget: &mut usize) -> AppResult<()> {
        if depth > 32 {
            return Err(AppError::InvalidInput);
        }
        let cost = match value {
            serde_json::Value::Null => 4,
            serde_json::Value::Bool(_) => 5,
            serde_json::Value::Number(_) => 24,
            serde_json::Value::String(v) => v.len().saturating_mul(6).saturating_add(2),
            serde_json::Value::Array(values) => {
                for v in values {
                    count(v, depth + 1, budget)?;
                }
                values.len().saturating_add(2)
            }
            serde_json::Value::Object(values) => {
                for (key, v) in values {
                    *budget = budget
                        .checked_sub(key.len().saturating_mul(6).saturating_add(4))
                        .ok_or(AppError::InvalidInput)?;
                    count(v, depth + 1, budget)?;
                }
                2
            }
        };
        *budget = budget.checked_sub(cost).ok_or(AppError::InvalidInput)?;
        Ok(())
    }
    count(value, 0, &mut 65536)
}
