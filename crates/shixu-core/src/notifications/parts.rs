use crate::contracts::notification::{PartResult, PartStatus};
/// Summary is a UI aggregate, never evidence that a message is a non-event.
/// Preserve a unanimous failure/pending state; mixed outcomes remain partial.
pub fn summarize_parts(parts: &[PartResult]) -> PartStatus {
    let Some(first) = parts.first() else {
        return PartStatus::Success;
    };
    if parts.iter().all(|p| p.status == first.status) {
        first.status
    } else {
        PartStatus::PartialParse
    }
}

use super::{limits::Resource, store::MessageStore};
use crate::{
    contracts::{AppResult, error::AppError, notification::*},
    storage::{Database, database::storage_error},
};
use rusqlite::{OptionalExtension, params};
use std::sync::Arc;
/// Durable worker coordination. No parser is launched by this queue.
pub struct TaskQueue {
    db: Arc<Database>,
}
/// The opaque token binds the descriptive fields to one claimed task.
pub struct TaskLease {
    pub message_key: MessageKey,
    pub revision: u64,
    pub part: MessagePart,
    token: String,
}
impl TaskQueue {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }
    pub fn enqueue(
        &self,
        key: &MessageKey,
        revision: u64,
        now: i64,
        limits: &ParserLimits,
    ) -> AppResult<u32> {
        if revision == 0 || revision > i64::MAX as u64 {
            return Err(AppError::InvalidInput);
        }
        self.db.transaction(|tx| {
   let payload:Option<Vec<u8>>=tx.query_row("SELECT payload FROM messages WHERE message_key=?1 AND revision=?2 AND revoked=0 AND payload IS NOT NULL",params![key.to_string(),revision as i64],|r|r.get(0)).optional().map_err(storage_error)?;
   let m:MessageEnvelope=self.db.unprotect(&payload.ok_or(AppError::Conflict)?)?;
   let parts:Vec<_>=m.parts.iter().filter(|p|p.kind!=PartKind::Text).collect();
   let mut sizes=Vec::new();let mut seen=std::collections::HashSet::new();
   for p in &parts {
    if !seen.insert(p.part_id){return Err(AppError::InvalidInput);}
    let resource=if p.kind==PartKind::Image {Resource::ImageBytes}else{Resource::FileBytes};
    // Reserve conservatively when the adapter has no trusted size yet.
    let size=p.byte_size.unwrap_or(if p.kind==PartKind::Image {limits.max_image_bytes.min(10*1024*1024)}else{limits.max_file_bytes.min(20*1024*1024)});
    limits.check(resource,size).map_err(|_|AppError::InvalidInput)?;sizes.push(size);
   }
   limits.check_message(&sizes).map_err(|_|AppError::InvalidInput)?;
   // Stale running jobs retain their slot until completion/recovery; deleting
   // them here could permit a second native child before the first terminates.
   tx.execute("DELETE FROM attachment_tasks WHERE state!='running' AND EXISTS(SELECT 1 FROM messages m WHERE m.message_key=attachment_tasks.message_key AND (m.revision!=attachment_tasks.revision OR m.revoked=1))",[]).map_err(storage_error)?;
   let mut added=0;
   for p in parts {
    added+=tx.execute("INSERT OR IGNORE INTO attachment_tasks(message_key,revision,part_id,state,due_at,retry_limit) VALUES (?1,?2,?3,'queued',?4,?5)",params![key.to_string(),revision as i64,p.part_id.to_string(),now,limits.max_download_retries.min(3)]).map_err(storage_error)?;
   }
   tx.execute("UPDATE attachment_tasks SET retry_limit=MIN(retry_limit,?3) WHERE message_key=?1 AND revision=?2",params![key.to_string(),revision as i64,limits.max_download_retries.min(3)]).map_err(storage_error)?;
   let count:i64=tx.query_row("SELECT count(*) FROM attachment_tasks WHERE state!='done'",[],|r|r.get(0)).map_err(storage_error)?;
   limits.check(Resource::PendingTasks,count as u64).map_err(|_|AppError::StorageFull)?;
   Ok(added as u32)
  })
    }
    pub fn claim(&self, now: i64, limits: &ParserLimits) -> AppResult<Option<TaskLease>> {
        self.db.transaction(|tx| {
   let active:i64=tx.query_row("SELECT count(*) FROM attachment_tasks WHERE state='running'",[],|r|r.get(0)).map_err(storage_error)?;
   if limits.check(Resource::ConcurrentParsers,active as u64+1).is_err(){return Ok(None);}
   tx.execute("UPDATE attachment_tasks SET retry_limit=MIN(retry_limit,?1) WHERE state='queued'",[limits.max_download_retries.min(3)]).map_err(storage_error)?;
   tx.execute("UPDATE attachment_tasks SET state='done' WHERE state='queued' AND retries>retry_limit",[]).map_err(storage_error)?;
   let row:Option<(String,i64,String,Vec<u8>)>=tx.query_row("SELECT t.message_key,t.revision,t.part_id,m.payload FROM attachment_tasks t JOIN messages m ON m.message_key=t.message_key AND m.revision=t.revision WHERE t.state='queued' AND t.due_at<=?1 AND m.revoked=0 AND m.payload IS NOT NULL ORDER BY t.due_at,t.message_key,t.part_id LIMIT 1",[now],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(storage_error)?;
   let Some((key,revision,pid,payload))=row else {return Ok(None);};
   let m:MessageEnvelope=self.db.unprotect(&payload)?;
   let part=m.parts.into_iter().find(|p|p.part_id.to_string()==pid).ok_or(AppError::Conflict)?;
   let token=uuid::Uuid::new_v4().to_string();
   tx.execute("UPDATE attachment_tasks SET state='running',lease=?4 WHERE message_key=?1 AND revision=?2 AND part_id=?3",params![key,revision,pid,token]).map_err(storage_error)?;
   Ok(Some(TaskLease{message_key:m.message_key,revision:revision as u64,part,token}))
  })
    }
    pub fn finish(
        &self,
        job: &TaskLease,
        result: PartResult,
        retryable: bool,
        now: i64,
    ) -> AppResult<()> {
        if result.part_id != job.part.part_id {
            return Err(AppError::InvalidInput);
        }
        self.db.transaction(|tx| {
   let policy:Option<(u32,u32)>=tx.query_row("SELECT retries,retry_limit FROM attachment_tasks WHERE message_key=?1 AND revision=?2 AND part_id=?3 AND state='running' AND lease=?4",params![job.message_key.to_string(),job.revision as i64,job.part.part_id.to_string(),job.token],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(storage_error)?;
   let (retries,retry_limit)=policy.ok_or(AppError::Conflict)?;
   // Only a transient acquisition failure is automatically retried. Auth,
   // expired references, unsupported input and partial originals are terminal.
   let retry=retryable && result.status==PartStatus::DownloadFailed && result.reason_code==Some(PartReason::DownloadUnavailable) && retries<retry_limit.min(3);
   let due=if retry {now.checked_add(i64::from(ParserLimits::v01().download_retry_delays_secs[retries as usize])*1000).ok_or(AppError::InvalidInput)?}else{now};
   MessageStore::new(self.db.clone()).record_parts_tx(tx,&job.message_key,job.revision,vec![result])?;
   tx.execute("UPDATE attachment_tasks SET state=?5,lease=NULL,retries=?6,due_at=?7 WHERE message_key=?1 AND revision=?2 AND part_id=?3 AND lease=?4",params![job.message_key.to_string(),job.revision as i64,job.part.part_id.to_string(),job.token,if retry {"queued"}else{"done"},retries+u32::from(retry),due]).map_err(storage_error)?;
   Ok(())
  })
    }
    /// Explicit retry of the same durable current task, within its original retry budget.
    /// Never creates calendar items, resets attempts, revives revoked sources or duplicates workers.
    pub fn retry_part(&self, part_id: PartId, now: i64) -> AppResult<()> {
        self.db.transaction(|tx|{
            let row:Option<(String,i64,String,u32,u32,Vec<u8>)>=tx.query_row("SELECT t.message_key,t.revision,t.state,t.retries,t.retry_limit,m.payload FROM attachment_tasks t JOIN messages m ON m.message_key=t.message_key AND m.revision=t.revision WHERE t.part_id=?1 AND m.revoked=0 AND m.payload IS NOT NULL",[part_id.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional().map_err(storage_error)?;
            let (key,revision,state,retries,limit,payload)=row.ok_or(AppError::Unsupported)?;
            let m:MessageEnvelope=self.db.unprotect(&payload)?;
            if let Some((_,config))=super::settings::current(&self.db,tx,m.source_id)? && (!config.enabled||!config.allowed_group_ids.contains(&m.group_id)){return Err(AppError::Conflict);}
            if state=="running" {return Err(AppError::Conflict);}
            if state=="queued" {return Ok(());}
            if retries>=limit.min(3){return Err(AppError::Unsupported);}
            let results:Option<Vec<u8>>=tx.query_row("SELECT payload FROM part_results WHERE message_key=?1 AND revision=?2",params![key,revision],|r|r.get(0)).optional().map_err(storage_error)?;
            let results:Vec<PartResult>=self.db.unprotect(&results.ok_or(AppError::Conflict)?)?;
            if !results.iter().any(|p|p.part_id==part_id&&p.status==PartStatus::DownloadFailed&&p.reason_code==Some(PartReason::DownloadUnavailable)){return Err(AppError::Unsupported);}
            tx.execute("UPDATE attachment_tasks SET state='queued',retries=retries+1,due_at=?4,lease=NULL WHERE message_key=?1 AND revision=?2 AND part_id=?3",params![key,revision,part_id.to_string(),now]).map_err(storage_error)?;
            Ok(())
        })
    }
    /// Startup recovery only after the native supervisor has proved every old child
    /// dead. Invalidates old leases; never call merely because a timer expired.
    pub fn recover_after_children_stopped(&self) -> AppResult<u64> {
        self.db.transaction(|tx| {
   tx.execute("DELETE FROM attachment_tasks WHERE EXISTS(SELECT 1 FROM messages m WHERE m.message_key=attachment_tasks.message_key AND (m.revision!=attachment_tasks.revision OR m.revoked=1))",[]).map_err(storage_error)?;
   Ok(tx.execute("UPDATE attachment_tasks SET state='queued',lease=NULL WHERE state='running'",[]).map_err(storage_error)? as u64)
  })
    }
}
