// Fixed private framed protocol. Never log input, error stacks, or library errors.
import fs from 'node:fs';
import { randomUUID, createHash } from 'node:crypto';
import { kdbx, preflight, validateText, MAX_FILE } from './crypto.mjs';
const MAX_FRAME = 1024 * 1024;
let db, stagedDigest, lastId = 0;
const codes = new Set(['LOCKED','AUTH_FAILED','UNSUPPORTED','CONFLICT','STORAGE_FULL','INVALID_INPUT']);
function fail(code) { throw Error(code); }
function credentials(master) { return new kdbx.KdbxCredentials(kdbx.ProtectedValue.fromString(validateText(master,true))); }
function readActive() {
  const stat = fs.lstatSync('vault.kdbx');
  if (!stat.isFile() || stat.isSymbolicLink() || stat.size > MAX_FILE) fail('AUTH_FAILED');
  return fs.readFileSync('vault.kdbx');
}
async function load(bytes, cred) { return kdbx.Kdbx.load(preflight(bytes),cred); }
function id(entry) {
  const h=Buffer.from(entry.uuid.id,'base64').toString('hex');
  return `${h.slice(0,8)}-${h.slice(8,12)}-${h.slice(12,16)}-${h.slice(16,20)}-${h.slice(20)}`;
}
function field(e,key) {const value=e.fields.get(key);return typeof value==='string'?value:value.getText();}
function summary(e) {
  const revision=Number(e.customData.get('shixu.revision')?.value);
  if (!Number.isSafeInteger(revision) || revision < 1) fail('UNSUPPORTED');
  return {entry_id:id(e),channel:field(e,'Title'),account:field(e,'UserName'),revision,created_at:e.times.creationTime.getTime(),updated_at:e.times.lastModTime.getTime()};
}
function entries() {
  const result=db.getDefaultGroup().entries;
  if(result.length>1000) fail('UNSUPPORTED');
  return result;
}
async function stage() {
  const saved=Buffer.from(await db.save());
  try {
    if(saved.length>MAX_FILE) fail('UNSUPPORTED');
    const verified=await load(saved,db.credentials);
    const expected=entries().map(summary);
    if(JSON.stringify(verified.getDefaultGroup().entries.map(summary))!==JSON.stringify(expected)) fail('AUTH_FAILED');
    // Verify all three fields, without emitting them.
    for(let i=0;i<entries().length;i++) {
      if(field(verified.getDefaultGroup().entries[i],'Password') !== field(entries()[i],'Password')) fail('AUTH_FAILED');
    }
    stagedDigest=createHash('sha256').update(saved).digest('hex');
    const fd=fs.openSync('vault.pending.kdbx','wx',0o600);
    try { fs.writeFileSync(fd,saved); } finally {fs.closeSync(fd);}
  } finally {saved.fill(0);}
}
function exact(obj, keys) {
  if(!obj || typeof obj!=='object' || Array.isArray(obj) || Object.keys(obj).sort().join()!==keys.sort().join()) fail('INVALID_INPUT');
}
async function operation(r) {
  const keys={create:['master'],open:['master'],list:[],reveal:['entry_id'],create_entry:['channel','account','password'],update:['entry_id','expected_revision','channel','account','password'],delete:['entry_id','expected_revision'],change_master:['current','next']};
  if(!Object.hasOwn(keys,r.op)) fail('UNSUPPORTED');
  exact(r,['v','id','op',...keys[r.op]]);
  if(r.v!==1 || !Number.isSafeInteger(r.id) || r.id!==lastId+1) fail('UNSUPPORTED');
  lastId=r.id;
  if(r.op==='create'||r.op==='open') {
    if(db) fail('CONFLICT');
    const cred=credentials(r.master);
    if(r.op==='create') {
      if(fs.existsSync('vault.kdbx')) fail('CONFLICT');
      db=kdbx.Kdbx.create(cred,'Shixu');db.setVersion(4);db.header.setKdf(kdbx.Consts.KdfId.Argon2id);db.header.compression=0;
      db.header.kdfParameters.set('M',kdbx.VarDictionary.ValueType.UInt64,kdbx.Int64.from(67108864));
      db.header.kdfParameters.set('I',kdbx.VarDictionary.ValueType.UInt64,kdbx.Int64.from(3));
      db.header.kdfParameters.set('P',kdbx.VarDictionary.ValueType.UInt32,1);
      db.meta.recycleBinEnabled=false;db.meta.recycleBinUuid=undefined;db.getDefaultGroup().groups=[];
      await stage();
    } else {
      const bytes=readActive();try {db=await load(bytes,cred);}finally{bytes.fill(0);}
      if(db.groups.length!==1 || db.getDefaultGroup().groups.length || db.binaries.getAll().length) fail('UNSUPPORTED');
      entries().forEach(e=>{summary(e);validateText(field(e,'Title'));validateText(field(e,'UserName'),true);validateText(field(e,'Password'),true);});
    }
    return r.op==='open'?{type:'unit'}:{type:'unit',staged_digest:stagedDigest};
  }
  if(!db) fail('LOCKED');
  if(r.op==='list') return {type:'list',value:entries().map(summary)};
  if(r.op==='change_master') {
    const current=credentials(r.current), next=credentials(r.next), bytes=readActive();
    try {await load(bytes,current);}finally{bytes.fill(0);}
    db.credentials=next;await stage();
    const staged=fs.readFileSync('vault.pending.kdbx');
    try {
      let oldOpened=false;
      try {await load(staged,current);oldOpened=true;}catch(error){if(error.code!=='InvalidKey') throw error;}
      // Equal master is a no-op credential change, not a false old-rejected claim.
      if(oldOpened) fail('INVALID_INPUT');
    }finally{staged.fill(0);}
    return r.op==='open'?{type:'unit'}:{type:'unit',staged_digest:stagedDigest};
  }
  let entry;
  if(r.op!=='create_entry') {
    if(typeof r.entry_id!=='string' || !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(r.entry_id)) fail('INVALID_INPUT');
    entry=entries().find(e=>id(e)===r.entry_id);if(!entry)fail('INVALID_INPUT');
  }
  if(r.op==='reveal') return {type:'secret',value:Array.from(new TextEncoder().encode(field(entry,'Password')))};
  if(r.op==='update'||r.op==='delete') {
    if(!Number.isSafeInteger(r.expected_revision)||r.expected_revision!==summary(entry).revision) fail('CONFLICT');
  }
  if(r.op==='delete') { const value=summary(entry);db.getDefaultGroup().entries.splice(entries().indexOf(entry),1);await stage();return {type:'summary',value,staged_digest:stagedDigest}; }
  validateText(r.channel);validateText(r.account,true);validateText(r.password,true);if(!r.channel.trim()||!r.account.trim())fail('INVALID_INPUT');
  if(r.op==='create_entry') {
    if(entries().length>=1000)fail('UNSUPPORTED');
    entry=db.createEntry(db.getDefaultGroup());
    const raw=Buffer.from(randomUUID().replaceAll('-',''),'hex');entry.uuid=new kdbx.KdbxUuid(raw.buffer.slice(raw.byteOffset,raw.byteOffset+16));raw.fill(0);
    entry.customData=new Map();entry.customData.set('shixu.revision',{value:'1'});
  } else {entry.customData.set('shixu.revision',{value:String(summary(entry).revision+1)});entry.times.update();}
  for(const key of ['creationTime','lastModTime'])entry.times[key]=new Date(Math.floor(entry.times[key].getTime()/1000)*1000);
  entry.fields.set('Title',kdbx.ProtectedValue.fromString(r.channel));entry.fields.set('UserName',kdbx.ProtectedValue.fromString(r.account));entry.fields.set('Password',kdbx.ProtectedValue.fromString(r.password));
  await stage();return {type:'summary',value:summary(entry),staged_digest:stagedDigest};
}
function reply(r,result) {
  const data=Buffer.from(JSON.stringify({v:1,id:r.id,...result}));
  if(data.length>MAX_FRAME) {data.fill(0);process.exit(1);}
  const header=Buffer.alloc(4);header.writeUInt32BE(data.length);
  try { for(const part of [header,data]) {let pos=0;while(pos<part.length)pos+=fs.writeSync(1,part,pos,part.length-pos);} } finally {data.fill(0);header.fill(0);}
}
// Exactly one frame admitted at a time; no in-helper request queue.
async function readExact(n) {
  const data=Buffer.alloc(n);let pos=0;
  try {
    while(pos<n) {const got=fs.readSync(0,data,pos,n-pos,null);if(!got) throw Error('EOF');pos+=got;}
    return data;
  }catch(error){data.fill(0);throw error;}
}
try {
  if(!process.permission || process.permission.has('net') || process.permission.has('child') || process.permission.has('worker') || process.permission.has('inspector') || process.permission.has('addons') || process.permission.has('wasi')) process.exit(1);
  while(true) {
    const header=await readExact(4),len=header.readUInt32BE();header.fill(0);
    if(len===0||len>MAX_FRAME)process.exit(1);
    const bytes=await readExact(len);let r;
    try {r=JSON.parse(new TextDecoder('utf-8',{fatal:true}).decode(bytes));}finally{bytes.fill(0);}
    try {reply(r,await operation(r));}catch(error) {
      const code=codes.has(error.message)?error.message:(error.code==='ENOSPC'?'STORAGE_FULL':error.code==='InvalidKey'||error.code==='FileCorrupt'?'AUTH_FAILED':'AUTH_FAILED');
      reply(r,{type:'error',value:code});db=undefined;break;
    }
  }
} catch {} // No diagnostic payload crosses the process boundary.
