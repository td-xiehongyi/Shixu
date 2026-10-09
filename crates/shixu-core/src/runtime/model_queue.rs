//! Three total send admissions, 60s and 300s between retries. This is the D5
//! model policy, distinct from the three attachment DOWNLOAD retries (1/5/30m).
//! Crashed admitted attempts consume budget; recovery never resets counters.
use crate::{
    calendar::{EventService, match_event},
    contracts::{AppResult, error::AppError, notification::*},
    notifications::{
        consent::{ConsentStore, authorize_request},
        model::{ModelRequest, ModelService, ModelTransport},
        settings,
    },
    storage::{Database, database::storage_error},
};
use rusqlite::{OptionalExtension, params};
use std::sync::Arc;
pub(super) fn enqueue(
    db: &Database,
    consent: &ConsentStore,
    m: &MessageEnvelope,
    now: i64,
) -> AppResult<()> {
    let consent_session = &consent.instance_id;
    let consent = consent.snapshot()?;
    if authorize_request(&consent, m, false).is_err() {
        return Ok(());
    }
    db.transaction(|tx|{
  let Some((epoch,source))=settings::current(db,tx,m.source_id)? else{return Err(AppError::Conflict);};
  if !source.enabled||!source.allowed_group_ids.contains(&m.group_id){return Ok(());}
  tx.execute("UPDATE model_attempts SET state='done' WHERE state='queued' AND EXISTS(SELECT 1 FROM messages m WHERE m.message_key=model_attempts.message_key AND (m.revision!=model_attempts.revision OR m.revoked=1))",[]).map_err(storage_error)?;
  let exists:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM model_attempts WHERE message_key=?1 AND revision=?2)",params![m.message_key.to_string(),m.revision as i64],|r|r.get(0)).map_err(storage_error)?;
  if exists{return Ok(());}
  let count:u32=tx.query_row("SELECT count(*) FROM model_attempts WHERE state!='done'",[],|r|r.get(0)).map_err(storage_error)?;
  if count>=100{return Err(AppError::StorageFull);}
  tx.execute("INSERT INTO model_attempts(message_key,revision,consent_revision,consent_session,source_epoch,state,due_at) VALUES (?1,?2,?3,?6,?4,'queued',?5)",params![m.message_key.to_string(),m.revision as i64,i64::try_from(consent.revision).map_err(|_|AppError::InvalidInput)?,epoch as i64,now,consent_session]).map_err(storage_error)?;Ok(())
 })
}
struct Admission<'a> {
    db: &'a Database,
    transport: &'a dyn ModelTransport,
    key: MessageKey,
    revision: i64,
    epoch: i64,
    now: i64,
}
impl ModelTransport for Admission<'_> {
    fn send(&self, request: &ModelRequest) -> AppResult<String> {
        // The live permit makes pause wait for any admitted finite transport call.
        let _permit = self.db.coordinator().enter()?;
        self.db.transaction(|tx|{
   let m=match_event::message(self.db,tx,self.key)?;
   let (epoch,c)=settings::current(self.db,tx,m.source_id)?.ok_or(AppError::Conflict)?;
   if m.revoked||m.revision as i64!=self.revision||epoch as i64!=self.epoch||!c.enabled||c.account_id!=m.account_id||!c.allowed_group_ids.contains(&m.group_id){return Err(AppError::Conflict);}
   let changed=tx.execute("UPDATE model_attempts SET attempts=attempts+1,due_at=?3+CASE attempts WHEN 0 THEN 60000 WHEN 1 THEN 300000 ELSE 0 END WHERE message_key=?1 AND revision=?2 AND state='running' AND attempts<3",params![self.key.to_string(),self.revision,self.now]).map_err(storage_error)?;
   if changed!=1{return Err(AppError::Conflict);}Ok(())
  })?;
        // Admission above is the linearization point; edits after admission cannot
        // unsend a request. No DB mutex is retained over provider latency. The durable
        // calendar boundary rejects any now-stale completion.
        self.transport.send(request)
    }
}
pub(super) fn run(
    db: Arc<Database>,
    consent: &ConsentStore,
    transport: &dyn ModelTransport,
    now: i64,
) -> AppResult<bool> {
    let row=db.transaction(|tx|{
  tx.execute("UPDATE model_attempts SET state='done' WHERE consent_session!=?1",[&consent.instance_id]).map_err(storage_error)?;
  // Serialized runner: previous calls returned, so their transports are stopped.
  tx.execute("UPDATE model_attempts SET state=CASE WHEN attempts>=3 THEN 'done' ELSE 'queued' END WHERE state='running'",[]).map_err(storage_error)?;
  let row:Option<(String,i64,i64,i64)>=tx.query_row("SELECT message_key,revision,consent_revision,source_epoch FROM model_attempts WHERE state='queued' AND attempts<3 AND due_at<=?1 ORDER BY due_at,message_key LIMIT 1",[now],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(storage_error)?;
  if let Some((key,revision,_,_))=&row {tx.execute("UPDATE model_attempts SET state='running' WHERE message_key=?1 AND revision=?2",params![key,revision]).map_err(storage_error)?;}Ok(row)
 })?;
    let Some((key, revision, consent_revision, epoch)) = row else {
        return Ok(false);
    };
    let key: MessageKey = key.parse()?;
    let result = (|| {
        if consent.snapshot()?.revision as i64 != consent_revision {
            return Err(AppError::Conflict);
        }
        let (m, mut source, context) = db.transaction(|tx| {
            let m = match_event::message(&db, tx, key)?;
            let (current_epoch, current) =
                settings::current(&db, tx, m.source_id)?.ok_or(AppError::Conflict)?;
            if m.revoked
                || m.revision as i64 != revision
                || current_epoch as i64 != epoch
                || !current.enabled
                || !current.allowed_group_ids.contains(&m.group_id)
            {
                return Err(AppError::Conflict);
            }
            let proof = settings::proof(&db, tx, &m)?;
            let context = m
                .reply_to
                .and_then(|key| match_event::message(&db, tx, key).ok())
                .into_iter()
                .collect::<Vec<_>>();
            Ok((m, proof, context))
        })?;
        // Body-only model scheduling is deliberate. Attachment text is never silently
        // added to a queued request; future attachment-model admission is a separate gate.
        source.enabled = true;
        let admission = Admission {
            db: &db,
            transport,
            key,
            revision,
            epoch,
            now,
        };
        let service = ModelService {
            consent,
            transport: &admission,
        };
        let prepared = service.prepare(&m, &[], &context, &source)?;
        let result = service.dispatch(prepared)?;
        EventService::new(db.clone()).apply_model(result.batch)?;
        Ok(result.retryable)
    })();
    let retry = matches!(result, Ok(true));
    db.transaction(|tx| {
        let attempts: u32 = tx
            .query_row(
                "SELECT attempts FROM model_attempts WHERE message_key=?1 AND revision=?2",
                params![key.to_string(), revision],
                |r| r.get(0),
            )
            .map_err(storage_error)?;
        let delay = match attempts {
            1 => 60_000,
            2 => 300_000,
            _ => 0,
        };
        let due = now.checked_add(delay).ok_or(AppError::InvalidInput)?;
        tx.execute(
            "UPDATE model_attempts SET state=?3,due_at=?4 WHERE message_key=?1 AND revision=?2",
            params![
                key.to_string(),
                revision,
                if retry && attempts < 3 {
                    "queued"
                } else {
                    "done"
                },
                due
            ],
        )
        .map_err(storage_error)?;
        Ok(())
    })?;
    // Revocation/staleness is an expected terminal admission result, not a retry.
    match result {
        Err(AppError::Conflict | AppError::InvalidInput) => Ok(true),
        Err(e) => Err(e),
        Ok(_) => Ok(true),
    }
}
