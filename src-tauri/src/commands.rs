//! Portable command policy. Context is supplied by the native calling WebView, never payload fields.
use crate::{
    app_state::AppState,
    wire::{self, WirePatch, WireUndo},
};
use serde::Deserialize;
use shixu_core::contracts::{AppResult, calendar::EventQuery, error::AppError};
pub const COMMANDS: &[&str] = &[
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
            "calendar_query" | "calendar_edit" | "calendar_undo" | "show_vault_window"
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
