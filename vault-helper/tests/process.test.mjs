import test from 'node:test';
import assert from 'node:assert/strict';
import {spawnSync,spawn} from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
const repo=fileURLToPath(new URL('../..',import.meta.url));
const resources=path.join(repo,'resources/vault-linux-x64');
const node=path.join(resources,'runtime/node');
const helper=path.join(resources,'helper/helper.mjs');
function flags(work) {return ['--permission','--no-addons','--no-warnings','--max-old-space-size=128','--allow-fs-read='+path.join(resources,'helper'),'--allow-fs-read='+work,'--allow-fs-write='+work];}
function frame(r) {const b=Buffer.from(JSON.stringify(r)),h=Buffer.alloc(4);h.writeUInt32BE(b.length);return Buffer.concat([h,b]);}
test('packaged Node26 actually denies network child worker inspector addons WASI and outside files',()=>{
 const result=spawnSync(node,['--permission','--no-addons','--no-warnings','--input-type=module','-e',`
 import fs from 'node:fs';import cp from 'node:child_process';import {Worker} from 'node:worker_threads';import inspector from 'node:inspector';import {WASI} from 'node:wasi';import net from 'node:net';
 function deny(fn,code='ERR_ACCESS_DENIED'){try{fn();process.exitCode=1;}catch(e){if(e.code!==code)process.exitCode=2;}}
 deny(()=>fs.readFileSync('/etc/passwd'));deny(()=>fs.writeFileSync('/tmp/shixu-permission-must-not-exist','PUBLIC'));
 deny(()=>cp.spawn('/bin/true'));deny(()=>new Worker('PUBLIC',{eval:true}));deny(()=>inspector.open(0));deny(()=>process.dlopen({exports:{}},'/tmp/no-addon.node'),'ERR_DLOPEN_DISABLED');deny(()=>new WASI({version:'preview1'}));
 try { const socket=net.connect({host:'127.0.0.1',port:9});socket.on('connect',()=>{process.exitCode=3;socket.destroy()});socket.on('error',e=>{if(e.code!=='ERR_ACCESS_DENIED')process.exitCode=4;}); }catch(e){if(e.code!=='ERR_ACCESS_DENIED')process.exitCode=5;}
 `],{env:{},timeout:5000});
 assert.equal(result.status,0, result.stderr.toString());assert.equal(result.stdout.length,0);
});
test('actual helper rejects versions schema ids oversized and malformed frames with fixed errors',()=>{
 const work=fs.mkdtempSync(path.join(os.tmpdir(),'shixu-protocol-'));
 try {
  for(const r of [{v:2,id:1,op:'list'},{v:1,id:2,op:'list'},{v:1,id:1,op:'list',extra:'SYNTHETIC-CANARY'},{v:1,id:1,op:'arbitrary',secret:'SYNTHETIC-CANARY'}]) {
   const result=spawnSync(node,[...flags(work),helper],{cwd:work,env:{},input:frame(r),timeout:5000});
   assert.equal(result.stderr.length,0);const response=JSON.parse(result.stdout.subarray(4));
   assert.equal(response.type,'error');assert.ok(['INVALID_INPUT','UNSUPPORTED'].includes(response.value));assert.equal(result.stdout.includes(Buffer.from('SYNTHETIC-CANARY')),false);
  }
  for(const data of [Buffer.from([0,16,0,1]),Buffer.from([0,0,0,0]),Buffer.from([0,0,0,9,123])]) {
   const result=spawnSync(node,[...flags(work),helper],{cwd:work,env:{},input:data,timeout:5000});assert.equal(result.stdout.length,0);assert.equal(result.stderr.length,0);
  }
 }finally{fs.rmSync(work,{recursive:true,force:true});}
});
function rpc(child,r) {
 return new Promise((resolve,reject)=>{
  const chunks=[];let count=0,expected;
  const timer=setTimeout(()=>{child.stdout.off('data',receive);reject(Error('timeout'));},5000);
  function receive(b) {chunks.push(b);count+=b.length;const data=Buffer.concat(chunks);if(count>=4)expected=4+data.readUInt32BE();if(expected&&count>=expected){clearTimeout(timer);child.stdout.off('data',receive);resolve(JSON.parse(data.subarray(4)));}}
  child.stdout.on('data',receive);child.stdin.write(frame(r));
 });
}
test('synthetic secrets stay out argv env errors work files; list excludes passwords and reveal selects one',async()=>{
 const work=fs.mkdtempSync(path.join(os.tmpdir(),'shixu-leak-'));
 const canaries=['SYNTHETIC-MASTER-PIPE','SYNTHETIC-ACCOUNT-PIPE','SYNTHETIC-PASSWORD-PIPE','SYNTHETIC-OTHER-PASSWORD'];
 const child=spawn(node,[...flags(work),helper],{cwd:work,env:{},stdio:['pipe','pipe','pipe']});
 let error=Buffer.alloc(0);child.stderr.on('data',b=>{error=Buffer.concat([error,b])});
 try {
  assert.equal((await rpc(child,{v:1,id:1,op:'create',master:canaries[0]})).type,'unit');fs.renameSync(path.join(work,'vault.pending.kdbx'),path.join(work,'vault.kdbx'));
  const first=await rpc(child,{v:1,id:2,op:'create_entry',channel:'public',account:canaries[1],password:canaries[2]});assert.equal(first.type,'summary');fs.renameSync(path.join(work,'vault.pending.kdbx'),path.join(work,'vault.kdbx'));
  const second=await rpc(child,{v:1,id:3,op:'create_entry',channel:'public',account:canaries[1],password:canaries[3]});assert.equal(second.type,'summary');fs.renameSync(path.join(work,'vault.pending.kdbx'),path.join(work,'vault.kdbx'));
  const listed=await rpc(child,{v:1,id:4,op:'list'});assert.equal(listed.value.length,2);
  assert.equal(JSON.stringify(listed).includes(canaries[2]),false);assert.equal(JSON.stringify(listed).includes(canaries[3]),false);
  const revealed=await rpc(child,{v:1,id:5,op:'reveal',entry_id:first.value.entry_id});assert.equal(revealed.type,'secret');assert.equal(Buffer.from(revealed.value).toString(),canaries[2]);
  for(const proc of ['cmdline','environ'])for(const canary of canaries)assert.equal(fs.readFileSync('/proc/'+child.pid+'/'+proc).includes(Buffer.from(canary)),false);
  assert.equal(fs.readFileSync('/proc/'+child.pid+'/environ').length,0);
  for(const file of fs.readdirSync(work))for(const canary of canaries)assert.equal(fs.readFileSync(path.join(work,file)).includes(Buffer.from(canary)),false);
  const rejected=await rpc(child,{v:1,id:6,op:'create_entry',channel:'public',account:canaries[1],password:canaries[2]+'\n'});assert.equal(rejected.value,'INVALID_INPUT');for(const canary of canaries)assert.equal(JSON.stringify(rejected).includes(canary),false);
  assert.equal(error.length,0);
 }finally{if(child.exitCode===null){const exited=new Promise(resolve=>child.once('exit',resolve));child.kill();await exited;}fs.rmSync(work,{recursive:true,force:true});}
});
