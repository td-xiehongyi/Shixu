import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';
import { spawnSync } from 'node:child_process';
import { kdbx, preflight } from '../../resources/vault-linux-x64/helper/crypto.mjs';
const repo=fileURLToPath(new URL('../..',import.meta.url));
const resources=path.join(repo,'resources/vault-linux-x64');
const require=createRequire(path.join(resources,'helper/crypto.mjs'));
const fflate=require('fflate');
function frame(r) {const bytes=Buffer.from(JSON.stringify(r)),h=Buffer.alloc(4);h.writeUInt32BE(bytes.length);return Buffer.concat([h,bytes]);}
function helper(work,requests) {
 const result=spawnSync(path.join(resources,'runtime/node'),['--permission','--no-addons','--no-warnings','--max-old-space-size=128','--allow-fs-read='+path.join(resources,'helper'),'--allow-fs-read='+work,'--allow-fs-write='+work,path.join(resources,'helper/helper.mjs')],{cwd:work,env:{},input:Buffer.concat(requests.map(frame)),timeout:5000});
 assert.equal(result.stderr.length,0);assert.equal(result.status,0);
 const replies=[];let pos=0;while(pos<result.stdout.length){const len=result.stdout.readUInt32BE(pos);assert.ok(len<=1024*1024);replies.push(JSON.parse(result.stdout.subarray(pos+4,pos+4+len)));pos+=4+len;}return replies;
}
function work() {return fs.mkdtempSync(path.join(repo,'.superpowers/sdd/shixu-v0.1/task-kdbxweb-engine-fix1-work-'));}
function makeDb() {
 const db=kdbx.Kdbx.create(new kdbx.KdbxCredentials(kdbx.ProtectedValue.fromString('SYNTHETIC-FIX1')),'Shixu');
 db.setVersion(4);db.header.setKdf(kdbx.Consts.KdfId.Argon2id);db.header.compression=0;
 db.header.kdfParameters.set('M',kdbx.VarDictionary.ValueType.UInt64,kdbx.Int64.from(8192));
 db.header.kdfParameters.set('I',kdbx.VarDictionary.ValueType.UInt64,kdbx.Int64.from(1));db.header.kdfParameters.set('P',kdbx.VarDictionary.ValueType.UInt32,1);
 db.meta.recycleBinEnabled=false;db.meta.recycleBinUuid=undefined;db.getDefaultGroup().groups=[];return db;
}
test('authenticated XML compressed binaries reject before any decompressor call',async()=>{
 const original=fflate.gunzipSync;let calls=0,expanded=0;
 fflate.gunzipSync=(...args)=>{calls++;const output=original(...args);expanded+=output.length;return output;};
 const dir=work();
 try {
  for(const entryPath of [false,true]) {
   const db=makeDb();if(entryPath)db.createEntry(db.getDefaultGroup());const build=db.buildXml.bind(db);
   const gzip=fflate.gzipSync(new Uint8Array(16*1024*1024));
   db.buildXml=ctx=>{build(ctx);const xml=db.xml,binary=xml.createElement('Binary');
    if(entryPath){const key=xml.createElement('Key'),value=xml.createElement('Value');key.textContent='attachment';value.setAttribute('Compressed','True');value.textContent=Buffer.from(gzip).toString('base64');binary.appendChild(key);binary.appendChild(value);xml.getElementsByTagName('Entry')[0].appendChild(binary);}
    else {const binaries=xml.createElement('Binaries');binary.setAttribute('ID','0');binary.setAttribute('Compressed','True');binary.textContent=Buffer.from(gzip).toString('base64');binaries.appendChild(binary);xml.getElementsByTagName('Meta')[0].appendChild(binaries);}
   };
   const bytes=Buffer.from(await db.save());assert.ok(bytes.length<65536);preflight(bytes);
   let rejection;try{await kdbx.Kdbx.load(preflight(bytes),db.credentials);}catch(error){rejection=error;}
   assert.equal(calls,0,'unsupported XML must not call gunzip');assert.equal(expanded,0);assert.match(rejection?.message??'',/UNSUPPORTED/);
   fs.writeFileSync(path.join(dir,'vault.kdbx'),bytes);const before=fs.readFileSync(path.join(dir,'vault.kdbx'));
   assert.equal(helper(dir,[{v:1,id:1,op:'open',master:'SYNTHETIC-FIX1'}])[0].value,'UNSUPPORTED');assert.deepEqual(fs.readFileSync(path.join(dir,'vault.kdbx')),before);
  }
 }finally{fflate.gunzipSync=original;fs.rmSync(dir,{recursive:true,force:true});}
});
test('all XML binary paths reject before upstream binary object loading',()=>{
 for(const xml of ['<KeePassFile><Meta><Binaries><Binary ID="0">AA==</Binary></Binaries></Meta></KeePassFile>','<KeePassFile><Root><Entry><Binary><Key>x</Key><Value Ref="0"/></Binary></Entry></Root></KeePassFile>','<x><n:Binary xmlns:n="urn:x"/></x>','<x><Value Compressed="True">AA==</Value></x>'])assert.throws(()=>new globalThis.DOMParser().parseFromString(xml,'application/xml'),/UNSUPPORTED/);
});
test('opening field-legal but over-budget list rejects without changing ciphertext',async()=>{
 const db=makeDb();for(let i=0;i<10;i++){const e=db.createEntry(db.getDefaultGroup());e.customData=new Map([['shixu.revision',{value:'1'}]]);e.fields.set('Title',kdbx.ProtectedValue.fromString('c'.repeat(65536)));e.fields.set('UserName',kdbx.ProtectedValue.fromString('a'.repeat(65536)));e.fields.set('Password',kdbx.ProtectedValue.fromString('synthetic'));}
 const bytes=Buffer.from(await db.save());assert.ok(bytes.length<8*1024*1024);const dir=work();
 try{fs.writeFileSync(path.join(dir,'vault.kdbx'),bytes);const replies=helper(dir,[{v:1,id:1,op:'open',master:'SYNTHETIC-FIX1'}]);assert.equal(replies[0].type,'error');assert.equal(replies[0].value,'UNSUPPORTED');assert.deepEqual(fs.readFileSync(path.join(dir,'vault.kdbx')),bytes);assert.equal(fs.existsSync(path.join(dir,'vault.pending.kdbx')),false);}finally{fs.rmSync(dir,{recursive:true,force:true});}
});
