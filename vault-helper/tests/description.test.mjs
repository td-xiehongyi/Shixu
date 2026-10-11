// Real helper + real encrypted KDBX. Functional test, not an OS sandbox proof.
import test from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { kdbx, preflight } from '../crypto.mjs';
const helper = fileURLToPath(new URL('../helper.mjs', import.meta.url));
const runtime = fileURLToPath(new URL(`../../resources/vault-${process.platform === 'win32' ? 'win' : 'linux'}-x64/runtime/${process.platform === 'win32' ? 'node.exe' : 'node'}`, import.meta.url));
function client(work) {
  const child = spawn(runtime, ['--permission', '--no-addons', '--no-warnings', '--max-old-space-size=128', `--allow-fs-read=${path.dirname(helper)}`, `--allow-fs-read=${work}`, `--allow-fs-write=${work}`, helper], { cwd: work, stdio: ['pipe', 'pipe', 'ignore'] });
  let id = 0;
  return {
    async request(op, args = {}) {
      const bytes = Buffer.from(JSON.stringify({ v: 1, id: ++id, op, ...args }));
      const header = Buffer.alloc(4); header.writeUInt32BE(bytes.length);
      return new Promise((resolve, reject) => {
        if (child.exitCode !== null || child.signalCode !== null) { reject(Error('helper exited')); return; }
        let data = Buffer.alloc(0);
        const cleanup = () => { clearTimeout(timer); child.stdout.off('data', receive); child.off('exit', exited); child.off('error', failed); };
        const timer = setTimeout(() => { cleanup(); reject(Error('helper timeout')); }, 15000);
        function exited() { cleanup(); reject(Error('helper exited')); }
        function failed(error) { cleanup(); reject(error); }
        function receive(part) { data = Buffer.concat([data, part]); if (data.length >= 4 && data.length >= 4 + data.readUInt32BE()) { cleanup(); resolve(JSON.parse(data.subarray(4))); } }
        child.once('exit', exited); child.once('error', failed);
        child.stdout.on('data', receive); child.stdin.write(Buffer.concat([header, bytes]));
      });
    },
    async close() { if (child.exitCode === null && child.signalCode === null) { const done = new Promise(r => child.once('exit', r)); child.kill(); await done; } },
  };
}
function commit(work) { fs.renameSync(path.join(work, 'vault.pending.kdbx'), path.join(work, 'vault.kdbx')); }
test('description preserves multiline text through encrypted create update reopen and clear; legacy Notes missing defaults empty', async () => {
  const work = fs.mkdtempSync(path.join(os.tmpdir(), 'shixu-description-'));
  const master = 'fictional-description-master';
  let c = client(work);
  try {
    assert.equal((await c.request('create', { master })).type, 'unit'); commit(work); await c.close();
    const credentials = new kdbx.KdbxCredentials(kdbx.ProtectedValue.fromString(master));
    const old = await kdbx.Kdbx.load(preflight(fs.readFileSync(path.join(work, 'vault.kdbx'))), credentials);
    const legacy = old.createEntry(old.getDefaultGroup());
    legacy.customData = new Map([['shixu.revision', { value: '1' }]]);
    legacy.fields.set('Title', kdbx.ProtectedValue.fromString('legacy'));
    legacy.fields.set('UserName', kdbx.ProtectedValue.fromString('legacy@example.invalid'));
    legacy.fields.set('Password', kdbx.ProtectedValue.fromString('fictional-old-password'));
    legacy.fields.delete('Notes');
    fs.writeFileSync(path.join(work, 'vault.kdbx'), Buffer.from(await old.save()));
    c = client(work); assert.equal((await c.request('open', { master })).type, 'unit');
    const oldRow = (await c.request('list')).value[0];
    assert.equal(oldRow.description, '');
    const description = '  虚构用途说明\r\n第二行 🗝️\n末尾空格  ';
    const created = await c.request('create_entry', { channel: 'fictional', account: 'fake@example.invalid', password: 'fictional-description-password', description });
    assert.equal(created.type, 'summary'); assert.equal(created.value.description, description); commit(work);
    const bytes = fs.readFileSync(path.join(work, 'vault.kdbx'));
    assert.equal(bytes.includes(Buffer.from(description)), false);
    const decoded = await kdbx.Kdbx.load(preflight(bytes), credentials);
    const notes = decoded.getDefaultGroup().entries[1].fields.get('Notes');
    assert.ok(notes instanceof kdbx.ProtectedValue); assert.equal(notes.getText(), description);
    await c.close(); c = client(work); assert.equal((await c.request('open', { master })).type, 'unit');
    assert.equal((await c.request('list')).value[1].description, description);
    const edited = '更新说明\n保留换行';
    const updated = await c.request('update', { entry_id: created.value.entry_id, expected_revision: created.value.revision, channel: 'fictional', account: 'fake@example.invalid', password: 'fictional-description-password', description: edited });
    assert.equal(updated.value.description, edited); commit(work);
    const cleared = await c.request('update', { entry_id: created.value.entry_id, expected_revision: updated.value.revision, channel: 'fictional', account: 'fake@example.invalid', password: 'fictional-description-password', description: '' });
    assert.equal(cleared.value.description, ''); commit(work);
    // Changing credentials after multiple staged edits must retain both entries.
    let current = master;
    for (let i = 0; i < 4; i++) {
      const next = current === master ? 'fictional-description-master-next' : master;
      const changed = await c.request('change_master', { current, next });
      assert.equal(changed.type, 'unit'); commit(work); current = next;
      assert.equal(Buffer.from((await c.request('reveal', {entry_id: created.value.entry_id})).value).toString(), 'fictional-description-password');
    }
    await c.close(); c = client(work); assert.equal((await c.request('open', { master })).type, 'unit');
    const final = (await c.request('list')).value;
    assert.equal(final[0].account, 'legacy@example.invalid'); assert.equal(final[0].description, ''); assert.equal(final[1].description, '');
  } finally { await c.close(); fs.rmSync(work, { recursive: true, force: true }); }
});

test('metadata-only update retains protected password across reopen; explicit change replaces it', async () => {
  const work = fs.mkdtempSync(path.join(os.tmpdir(), 'shixu-edit-password-'));
  let c = client(work); const master = 'fictional-edit-master';
  try {
    assert.equal((await c.request('create', {master})).type, 'unit'); commit(work);
    const created = await c.request('create_entry', {channel:'original',account:'test@example.invalid',password:'original-secret',description:'old'});
    assert.equal(created.type,'summary'); commit(work);
    const update={entry_id:created.value.entry_id,expected_revision:created.value.revision,channel:'edited',account:'test@example.invalid',description:'changed without password'};
    const edited=await c.request('update',update);assert.equal(edited.type,'summary');commit(work);
    await c.close();c=client(work);assert.equal((await c.request('open',{master})).type,'unit');
    assert.equal(Buffer.from((await c.request('reveal',{entry_id:update.entry_id})).value).toString(),'original-secret');
    const changed=await c.request('update',{...update,expected_revision:edited.value.revision,password:'new-secret'});assert.equal(changed.type,'summary');commit(work);
    await c.close();c=client(work);assert.equal((await c.request('open',{master})).type,'unit');
    assert.equal(Buffer.from((await c.request('reveal',{entry_id:update.entry_id})).value).toString(),'new-secret');
    const before=fs.readFileSync(path.join(work,'vault.kdbx'));
    const empty=await c.request('update',{...update,expected_revision:changed.value.revision,password:''});assert.equal(empty.type,'error');
    assert.deepEqual(fs.readFileSync(path.join(work,'vault.kdbx')),before);
  } finally { await c.close(); fs.rmSync(work,{recursive:true,force:true}); }
});
