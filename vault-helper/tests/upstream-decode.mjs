// Independent published upstream bundle decoder, test only, secrets via stdin.
import fs from 'node:fs';
import {createRequire} from 'node:module';
import {argon2id} from 'hash-wasm';
import {DOMParser,XMLSerializer} from '@xmldom/xmldom';
const require=createRequire(import.meta.url);
const upstream=require('kdbxweb');
globalThis.DOMParser=class{parseFromString(xml,type){if(/<!DOCTYPE|<!ENTITY/i.test(xml))throw Error('rejected');return new DOMParser({onError(){throw Error('rejected')}}).parseFromString(xml,type)}};
globalThis.XMLSerializer=XMLSerializer;
upstream.CryptoEngine.setArgon2Impl(async(password,salt,memory,iterations,hashLength,parallelism,type,version)=>{
 if(type!==2||version!==19||memory>65536||iterations>4)throw Error('rejected');
 const out=await argon2id({password:new Uint8Array(password),salt:new Uint8Array(salt),memorySize:memory,iterations,hashLength,parallelism,outputType:'binary'});return out.buffer.slice(out.byteOffset,out.byteOffset+out.length);
});
try {
 const input=JSON.parse(fs.readFileSync(0,'utf8'));
 const bytes=fs.readFileSync(input.path);
 const db=await upstream.Kdbx.load(bytes.buffer.slice(bytes.byteOffset,bytes.byteOffset+bytes.byteLength),new upstream.KdbxCredentials(upstream.ProtectedValue.fromString(input.master)));
 const entries=db.getDefaultGroup().entries;
 if(entries.length!==input.entries.length)process.exit(1);
 for(let i=0;i<entries.length;i++){
  const e=entries[i],expected=input.entries[i];
  for(const [field,key] of [['Title','channel'],['UserName','account'],['Password','password']])if(e.fields.get(field).getText()!==expected[key])process.exit(2);
  if(Buffer.from(e.uuid.id,'base64').toString('hex')!==expected.id.replaceAll('-',''))process.exit(3);
 }
 process.stdout.write('UPSTREAM_DECODE_OK');
}catch{process.exit(4);}
