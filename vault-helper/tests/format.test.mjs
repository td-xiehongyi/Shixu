import test from 'node:test';
import assert from 'node:assert/strict';
import { kdbx,preflight,validateText } from '../../resources/vault-linux-x64/helper/crypto.mjs';
function db() {
 const db=kdbx.Kdbx.create(new kdbx.KdbxCredentials(kdbx.ProtectedValue.fromString('SYNTHETIC-ONLY')),'Shixu');
 db.setVersion(4);db.header.setKdf(kdbx.Consts.KdfId.Argon2id);db.header.compression=0;
 db.header.kdfParameters.set('M',kdbx.VarDictionary.ValueType.UInt64,kdbx.Int64.from(8192));
 db.header.kdfParameters.set('I',kdbx.VarDictionary.ValueType.UInt64,kdbx.Int64.from(1));db.header.kdfParameters.set('P',kdbx.VarDictionary.ValueType.UInt32,1);return db;
}
test('real compressed KDBX input rejected before decompression',async()=>{
 const source=db();source.header.compression=1;const data=Buffer.from(await source.save());assert.throws(()=>preflight(data),/UNSUPPORTED/);
});
test('header preflight rejects hostile memory version secret associated-data and unknown KDF',async()=>{
 const source=db(), valid=Buffer.from(await source.save());assert.doesNotThrow(()=>preflight(valid));
 for(const [key,type,val] of [['P',12,1],['V',12,19],['M',13,kdbx.Int64.from(8192)],['M',5,kdbx.Int64.from(67109888)],['V',4,16],['I',5,kdbx.Int64.from(5)],['P',4,2],['K',66,new ArrayBuffer(0)],['A',66,new ArrayBuffer(0)],['$UUID',66,new ArrayBuffer(16)]]) {
  const db2=db();db2.header.kdfParameters.set(key,type,val);
  // Header-only serialization invokes no hostile KDF.
  db2.header.generateSalts();const stm=new kdbx.BinaryStream();db2.header.write(stm);const bytes=Buffer.from(stm.getWrittenBytes());
  assert.throws(()=>preflight(bytes),/UNSUPPORTED/);
 }
 assert.throws(()=>preflight(Buffer.alloc(8*1024*1024+1)),/AUTH_FAILED/);
 const malformed=Buffer.from(valid);malformed.writeUInt32LE(0xffffffff,13);assert.throws(()=>preflight(malformed),/AUTH_FAILED/);
});
test('strict xmldom rejects DTD entities and malformed XML while Unicode stays exact',()=>{
 for(const xml of ['<!DOCTYPE x [<!ENTITY secret SYSTEM "file:///etc/passwd">]><x>&secret;</x>','<x><y></x>','<x>&unknown;</x>'])assert.throws(()=>new globalThis.DOMParser().parseFromString(xml,'application/xml'));
 assert.equal(new globalThis.DOMParser().parseFromString('<x> 中文 🗝️ </x>','application/xml').documentElement.textContent,' 中文 🗝️ ');
 for(const nl of ['\r','\n','\u0085','\u2028','\u2029'])assert.throws(()=>validateText('a'+nl+'b',true),/INVALID_INPUT/);
 assert.equal(validateText('c\r\n\u0085\u2028\u2029',false),'c\r\n\u0085\u2028\u2029');
});
