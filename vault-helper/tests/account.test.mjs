import test from 'node:test';
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
const repo=fileURLToPath(new URL('../..',import.meta.url));
const resources=path.join(repo,'resources/vault-linux-x64');
function client(work) {
 const child=spawn(path.join(resources,'runtime/node'),['--permission','--no-addons','--no-warnings','--max-old-space-size=128','--allow-fs-read='+path.join(resources,'helper'),'--allow-fs-read='+work,'--allow-fs-write='+work,path.join(resources,'helper/helper.mjs')],{cwd:work,env:{},stdio:['pipe','pipe','ignore']});
 let id=0;
 return {async request(op,args={}) {
  const bytes=Buffer.from(JSON.stringify({v:1,id:++id,op,...args})),header=Buffer.alloc(4);header.writeUInt32BE(bytes.length);
  return new Promise((resolve,reject)=>{
   let data=Buffer.alloc(0);const timer=setTimeout(()=>{child.stdout.off('data',receive);reject(Error('helper timeout'));},5000);
   function receive(part) {data=Buffer.concat([data,part]);if(data.length>=4&&data.length>=4+data.readUInt32BE()){clearTimeout(timer);child.stdout.off('data',receive);resolve(JSON.parse(data.subarray(4)));}}
   child.stdout.on('data',receive);child.stdin.write(Buffer.concat([header,bytes]));
  });
 },async close(){if(child.exitCode===null&&child.signalCode===null){const exited=new Promise(r=>child.once('exit',r));child.kill();await exited;}}};
}
function work(){return fs.mkdtempSync(path.join(repo,'.superpowers/sdd/shixu-v0.1/task-windows-vault-wiring-helper-'));}
function commit(dir){fs.renameSync(path.join(dir,'vault.pending.kdbx'),path.join(dir,'vault.kdbx'));}
test('actual helper account permits exact all-newline Unicode NUL whitespace through create save reopen',async()=>{
 const dir=work(), account=' 用户\r单独\r\n行\n\u0085\u2028\u2029/"\0 🗝️ '; let c=client(dir);
 try {
  assert.equal((await c.request('create',{master:'synthetic master'})).type,'unit');commit(dir);
  const created=await c.request('create_entry',{channel:' channel\r\n ',account,password:' synthetic password '});
  assert.equal(created.type,'summary');assert.equal(created.value.account,account);commit(dir);
  await c.close();c=client(dir);assert.equal((await c.request('open',{master:'synthetic master'})).type,'unit');
  const listed=await c.request('list');assert.equal(listed.value[0].account,account);
  const reveal=await c.request('reveal',{entry_id:created.value.entry_id});assert.equal(Buffer.from(reveal.value).toString(),' synthetic password ');
  const updatedAccount=account+' updated\r\n ';const updated=await c.request('update',{entry_id:created.value.entry_id,expected_revision:created.value.revision,channel:' channel\r\n ',account:updatedAccount,password:'new password'});
  assert.equal(updated.type,'summary');assert.equal(updated.value.account,updatedAccount);commit(dir);await c.close();c=client(dir);
  assert.equal((await c.request('open',{master:'synthetic master'})).type,'unit');assert.equal((await c.request('list')).value[0].account,updatedAccount);
 }finally{await c.close();fs.rmSync(dir,{recursive:true,force:true});}
});
test('actual helper keeps all five newline guards on master current next and password without writing',async()=>{
 const dir=work(); let c=client(dir);
 try {
  assert.equal((await c.request('create',{master:'synthetic master'})).type,'unit');commit(dir);await c.close();
  const original=fs.readFileSync(path.join(dir,'vault.kdbx'));
  for(const nl of ['\r','\n','\u0085','\u2028','\u2029']) {
   for(const [op,args,open] of [
    ['create',{master:'m'+nl+'x'},false],
    ['open',{master:'m'+nl+'x'},false],
    ['change_master',{current:'m'+nl+'x',next:'next'},true],
    ['change_master',{current:'synthetic master',next:'m'+nl+'x'},true],
    ['create_entry',{channel:'channel',account:' account\r\n ',password:'p'+nl+'x'},true],
   ]) {
    c=client(dir);if(open)assert.equal((await c.request('open',{master:'synthetic master'})).type,'unit');
    const reply=await c.request(op,args);assert.equal(reply.type,'error');assert.equal(reply.value,'INVALID_INPUT');await c.close();
    assert.deepEqual(fs.readFileSync(path.join(dir,'vault.kdbx')),original);assert.equal(fs.existsSync(path.join(dir,'vault.pending.kdbx')),false);
   }
  }
 }finally{await c.close();fs.rmSync(dir,{recursive:true,force:true});}
});
