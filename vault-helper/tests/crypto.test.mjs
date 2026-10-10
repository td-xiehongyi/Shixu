import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
const wasm = createRequire(import.meta.url)('../../resources/vault-linux-x64/helper/node_modules/hash-wasm');
import { argon2Adapter } from '../../resources/vault-linux-x64/helper/crypto.mjs';
test('upstream Argon2id vector and fixed adapter32byte regression', async () => {
  // https://github.com/Daninet/hash-wasm/blob/v4.12.0/test/argon2.test.ts
  assert.equal(await wasm.argon2id({password:'a',salt:'abcdefgh',parallelism:1,iterations:2,memorySize:16,hashLength:16,outputType:'hex'}), 'f94aa50873d67fdd589d6774b87c0634');
  const value = await argon2Adapter(new TextEncoder().encode('password').buffer, new TextEncoder().encode('somesalt').buffer, 65536, 2, 32, 1, 2, 0x13);
  assert.equal(Buffer.from(value).toString('hex'), '09316115d5cf24ed5a15a31a3ba326e5cf32edc24702987c02b6566f61913cf7');
});
test('unsupported version type and attacker resource bounds rejected', async () => {
  for (const [m,i,l,p,t,v] of [[65536,2,32,1,2,16],[65536,2,32,1,0,19],[65537,2,32,1,2,19],[65536,5,32,1,2,19],[8,1,32,2,2,19],[65536,1,31,1,2,19]]) {
    await assert.rejects(argon2Adapter(new ArrayBuffer(32),new ArrayBuffer(32),m,i,l,p,t,v), /UNSUPPORTED/);
  }
});
