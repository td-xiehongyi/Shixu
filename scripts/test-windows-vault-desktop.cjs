// Actual Windows WebView2 integration, fixed fictional mode; no mocked IPC/crypto.
// Uses an already installed Playwright via SHIXU_PLAYWRIGHT_MODULE. No downloads.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const net = require('node:net');
const { spawn, execFileSync } = require('node:child_process');
const { chromium } = require(process.env.SHIXU_PLAYWRIGHT_MODULE || 'playwright');
assert.equal(process.platform, 'win32');
const root = path.resolve(__dirname, '..');
const exe = path.join(root, 'target/x86_64-pc-windows-msvc/debug/shixu-desktop.exe');
const out = process.env.SHIXU_DESKTOP_EVIDENCE_DIR || path.join(root, 'docs/verification/local/vault-click-master-2026-10-11');
const encrypted = path.join(path.dirname(exe), 'shixu-fictional-desktop/vault/active/vault.kdbx');
const master1 = 'Fictional-Desktop-Master-2026-A';
const master2 = 'Fictional-Desktop-Master-2026-B';
const entryPassword = 'FICTIONAL-ONLY-desktop-password';
const channel = `虚构桌面验收 ${Date.now()}`;
const account = 'fictional@example.invalid\n仅测试';
const description = '  虚构用途说明\n第二行 🗝️  ';
const editedDescription = '已编辑的虚构描述\n多行说明';
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const result = { kind: 'actual-windows-webview2-native', passed: [], release: 'BLOCKED' };
let child, browser, main, vault;
function passed(name) { result.passed.push(name); console.log(`PASS ${name}`); }
async function until(check, timeout = 45000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) { if (await check()) return; await sleep(100); }
  throw Error('Timed out waiting for native/UI state');
}
async function state(value) {
  await until(async () => (await vault.locator('.vault-state').textContent()) === value);
}
function clipboard() {
  // Called only after this test has confirmed its own fictional clipboard write.
  return execFileSync('powershell.exe', ['-NoProfile', '-STA', '-Command',
    '$OutputEncoding=[Console]::OutputEncoding=[Text.UTF8Encoding]::new(); [Console]::Write((Get-Clipboard -Raw))'], {encoding: 'utf8', windowsHide: true});
}
async function launch() {
  // Refuse any existing listener before launching; never attach to another app.
  await new Promise((resolve, reject) => {
    const probe = net.createServer();
    probe.once('error', () => reject(Error('Fictional test CDP port already in use')));
    probe.listen({host: '127.0.0.1', port: 9247, exclusive: true}, () => probe.close(resolve));
  });
  child = spawn(exe, ['--vault-fictional'], { cwd: path.dirname(exe), windowsHide: true,
    env: { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: '--remote-debugging-port=9247 --remote-debugging-address=127.0.0.1' } });
  child.stdout.pipe(fs.createWriteStream(path.join(out, 'desktop-native-stdout.log'), {flags: 'a'}));
  child.stderr.pipe(fs.createWriteStream(path.join(out, 'desktop-native-stderr.log'), {flags: 'a'}));
  await until(async () => {
    if (child.exitCode !== null) throw Error(`Desktop exited ${child.exitCode}`);
    try { browser = await chromium.connectOverCDP('http://127.0.0.1:9247'); return true; } catch { return false; }
  });
  const ownership = `$owners=@(Get-NetTCPConnection -State Listen -LocalPort 9247 -ErrorAction Stop | Select-Object -ExpandProperty OwningProcess -Unique)
$owned=$owners.Count -gt 0
foreach($listenerOwner in $owners) {
  $ancestorId=[uint32]$listenerOwner; $match=$false
  for($depth=0;$depth -lt 32;$depth++) {
    if($ancestorId -eq ${child.pid}) {$match=$true;break}
    $ancestor=Get-CimInstance Win32_Process -Filter "ProcessId=$ancestorId" -ErrorAction Stop
    if(!$ancestor -or $ancestor.ParentProcessId -eq 0) {break}
    $ancestorId=[uint32]$ancestor.ParentProcessId
  }
  if(!$match) {$owned=$false}
}
[Console]::Write($owned)`;
  assert.ok(execFileSync('powershell.exe', ['-NoProfile', '-Command', ownership], {encoding: 'utf8', windowsHide: true}).trim() === 'True', 'CDP listener must belong to this test child');
  await until(async () => {
    main = browser.contexts().flatMap(c => c.pages()).find(p => p.url().includes('?fictional=1'));
    return !!main;
  });
  assert.ok(main, 'must attach only to the dedicated fictional native window');
  await openVault();
}
async function openVault() {
  await main.getByRole('button', {name: '密码库', exact: true}).click();
  await until(async () => {
    vault = browser.contexts().flatMap(c => c.pages()).find(p => p.url().includes('window=vault&fictional=1'));
    return !!vault;
  });
  vault.setDefaultTimeout(45000);
  await state('已锁定');
  await vault.getByRole('button', {name: '解锁密码库', exact: true}).waitFor({state: 'visible'});
  await until(async () => await vault.getByRole('button', {name: '解锁密码库', exact: true}).isEnabled());
}
async function unlock(master) {
  await vault.getByLabel('主密码', {exact: true}).fill(master);
  await vault.getByRole('button', {name: '解锁密码库', exact: true}).click();
  await state('已解锁');
  await until(async () => await vault.getByRole('button', {name: '新增条目', exact: true}).isEnabled());
}
async function wrong(master) {
  await vault.getByLabel('主密码', {exact: true}).fill(master);
  await vault.getByRole('button', {name: '解锁密码库', exact: true}).click();
  await vault.getByText('主密码错误，密码库仍已锁定。', {exact: true}).waitFor();
  await state('已锁定');
  assert.equal(await vault.getByLabel('主密码', {exact: true}).inputValue(), '');
}
async function checkHeaderAfterScroll() {
  // Real wheel input must scroll the content, without moving native-view controls.
  const size = await vault.evaluate(() => ({width: innerWidth, height: innerHeight}));
  await vault.mouse.move(size.width - 40, size.height - 80);
  await vault.mouse.wheel(0, 1200);
  // WebView2 can ignore wheel events when the host is backgrounded. Also exercise
  // browser scrolling directly; subsequent clicks still use normal pointer input.
  await vault.evaluate(() => {
    document.scrollingElement?.scrollTo(0, document.documentElement.scrollHeight);
    const body = document.querySelector('.vault-body');
    body?.scrollTo(0, body.scrollHeight);
  });
  await sleep(200);
  await vault.mouse.wheel(0, 1200);
  await sleep(400);
  const geometry = await vault.getByRole('button', {name: '锁定密码库', exact: true}).evaluate(button => {
    const r = button.getBoundingClientRect();
    const body = document.querySelector('.vault-body');
    return {rootScroll: window.scrollY, top: r.top, bottom: r.bottom, height: innerHeight,
      contentHeight: body.scrollHeight, contentViewport: body.clientHeight, contentScroll: body.scrollTop,
      hit: document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2)?.closest('button') === button};
  });
  assert.equal(geometry.rootScroll, 0, 'content scrolling must not move the document viewport');
  assert.ok(geometry.top >= 0 && geometry.bottom <= geometry.height && geometry.hit, 'lock control must remain visible and receive pointer input');
  if (geometry.contentHeight > geometry.contentViewport) assert.ok(geometry.contentScroll > 0, 'overflowing content must remain scrollable');
  result.viewportChecks ??= [];
  result.viewportChecks.push(geometry);
}
async function stopOwned() {
  if (browser) { await browser.close().catch(() => {}); browser = null; }
  if (child && child.exitCode === null) {
    const exited = new Promise(resolve => child.once('exit', resolve));
    child.kill(); await exited;
  }
  await sleep(1200);
}
function nativeWindow(action, width=1487, height=900) {
  const code=`Add-Type @'
using System; using System.Runtime.InteropServices;
public class ShixuWindowTest {
public delegate bool Callback(IntPtr h, IntPtr p);
[DllImport("user32.dll")] public static extern bool EnumWindows(Callback c, IntPtr p);
[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint p);
[DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
[DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h,uint m,IntPtr w,IntPtr l);
[DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h,int c);
[DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
[DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h,IntPtr after,int x,int y,int w,int z,uint f);
[DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h,System.Text.StringBuilder title,int count);
public static int Run(uint pid,string action,int w,int z) {int n=0;EnumWindows((h,p)=>{uint id;GetWindowThreadProcessId(h,out id);var title=new System.Text.StringBuilder(256);GetWindowText(h,title,256);if(id==pid && title.ToString().StartsWith("拾序") && (action=="restore" || IsWindowVisible(h))) {n++; if(action=="close")PostMessage(h,0x10,IntPtr.Zero,IntPtr.Zero); if(action=="restore"){ShowWindow(h,9);SetForegroundWindow(h);}if(action=="minimize")ShowWindow(h,6);if(action=="maximize")ShowWindow(h,3);if(action=="resize")SetWindowPos(h,IntPtr.Zero,0,0,w,z,0x16);}return true;},IntPtr.Zero);return n;}
}
'@
[Console]::Write([ShixuWindowTest]::Run(${child.pid},'${action}',${width},${height}))`;
  return Number(execFileSync('powershell.exe',['-NoProfile','-Command',code],{encoding:'utf8',windowsHide:true}).trim());
}
function screenshotNative(name) {
  const code=`Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;using System.Runtime.InteropServices;using System.Text;
public class ShixuCapture {
public delegate bool Callback(IntPtr h,IntPtr p);
[StructLayout(LayoutKind.Sequential)] public struct Rect {public int L,T,R,B;}
[DllImport("user32.dll")] public static extern bool EnumWindows(Callback c,IntPtr p);
[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint p);
[DllImport("user32.dll",CharSet=CharSet.Unicode)]public static extern int GetWindowText(IntPtr h,StringBuilder b,int c);
[DllImport("user32.dll")]public static extern bool GetWindowRect(IntPtr h,out Rect r);
[DllImport("user32.dll")]public static extern IntPtr GetForegroundWindow();
[DllImport("user32.dll")]public static extern bool SetProcessDPIAware();
public static Rect Find(uint pid){SetProcessDPIAware();Rect found=new Rect();EnumWindows((h,p)=>{uint id;GetWindowThreadProcessId(h,out id);var title=new StringBuilder(256);GetWindowText(h,title,256);if(id==pid&&title.ToString().StartsWith("拾序")&&GetForegroundWindow()==h)GetWindowRect(h,out found);return true;},IntPtr.Zero);return found;}
}
'@
$r=[ShixuCapture]::Find(${child.pid})
if($r.R -le $r.L){throw 'Test window is not foreground'}
$b=[Drawing.Bitmap]::new($r.R-$r.L,$r.B-$r.T);$g=[Drawing.Graphics]::FromImage($b)
try{$g.CopyFromScreen($r.L,$r.T,0,0,$b.Size);$b.Save('${path.join(out,name).replaceAll("'","''")}')}finally{$g.Dispose();$b.Dispose()}`;
  execFileSync('powershell.exe',['-NoProfile','-Command',code],{windowsHide:true});
}
(async () => {
  fs.mkdirSync(out, {recursive: true});
  const identity = JSON.parse(execFileSync(exe, ['--release-manifest'], {encoding: 'utf8', windowsHide: true}));
  fs.writeFileSync(path.join(out, 'desktop-artifact-manifest.json'), JSON.stringify(identity, null, 2));
  result.artifactSha256 = identity.artifact_sha256;
  result.sourceSha256 = identity.manifest.source_sha256;
  const exists = fs.existsSync(encrypted);
  if (exists) fs.copyFileSync(encrypted, path.join(out, 'fictional-before.kdbx'));
  await launch();
  assert.equal(await main.evaluate(()=>window.__TAURI_INTERNALS__.invoke('get_vault_lock_state',{})),false);
  await vault.evaluate(()=>window.dispatchEvent(new Event('shixu-vault-view-failed')));
  await vault.getByRole('button',{name:'重试密码库',exact:true}).waitFor();
  await until(async()=>await main.getByText('密码库暂不可用，请在密码区点击重试。',{exact:true}).count()>0);
  await vault.getByRole('button',{name:'重试密码库',exact:true}).click();
  await until(async()=>await vault.getByRole('button',{name:'解锁密码库',exact:true}).isEnabled());
  passed('native-ready-failure-redacts-displays-error-and-explicit-retry-recovers');
  const denied = await main.evaluate(async () => {
    try { await window.__TAURI_INTERNALS__.invoke('vault_list', {}); return 'UNEXPECTED_SUCCESS'; }
    catch (error) { return String(error); }
  });
  assert.notEqual(denied, 'UNEXPECTED_SUCCESS');
  assert.match(denied, /AUTH_FAILED|not allowed|denied/i);
  passed('main-window-vault-IPC-denied');
  const reverse=await vault.evaluate(async()=>{try{await window.__TAURI_INTERNALS__.invoke('get_vault_lock_state',{});return 'UNEXPECTED_SUCCESS';}catch(e){return String(e);}});
  assert.match(reverse,/AUTH_FAILED|not allowed|denied/i);passed('vault-webview-main-IPC-denied');
  assert.equal(await vault.getByRole('button', {name: '忘记主密码', exact: true}).count(), 0);
  assert.equal(await vault.getByText('主密码无法找回，请妥善保管。', {exact: true}).count(), 0);
  await vault.getByRole('button', {name: '新建密码库', exact: true}).click();
  await vault.getByText('主密码无法找回，请妥善保管。', {exact: true}).waitFor();
  await vault.getByRole('button', {name: '返回解锁', exact: true}).click();
  passed('simplified-locked-and-creation-only-guidance');
  let master = master1;
  if (!exists) {
    await vault.getByRole('button', {name: '新建密码库', exact: true}).click();
    await vault.getByLabel('主密码', {exact: true}).fill(master);
    await vault.getByLabel('再次输入主密码', {exact: true}).fill(master);
    await vault.getByRole('button', {name: '创建密码库', exact: true}).click();
    await state('已解锁');
    passed('native-create-encrypted-vault');
  } else {
    // Reruns can only use this test's two fixed fictional masters. Never reset data.
    await vault.getByLabel('主密码', {exact: true}).fill(master2);
    await vault.getByRole('button', {name: '解锁密码库', exact: true}).click();
    await until(async () => (await vault.locator('.vault-state').textContent()) === '已解锁' || await vault.getByText('主密码错误，密码库仍已锁定。', {exact: true}).isVisible());
    if ((await vault.locator('.vault-state').textContent()) === '已解锁') master = master2;
    else await unlock(master1);
    passed('existing-fictional-vault-preserved');
  }
  assert.equal(await vault.getByRole('button', {name: '忘记主密码', exact: true}).count(), 0);
  await checkHeaderAfterScroll();
  await vault.getByRole('button', {name: '锁定密码库', exact: true}).click();
  await state('已锁定'); await unlock(master);
  passed('default-window-scroll-and-pointer-lock');
  await vault.getByRole('button', {name: '新增条目', exact: true}).click();
  await vault.getByLabel('渠道', {exact: true}).fill(channel);
  await vault.getByLabel('账号', {exact: true}).fill(account);
  await vault.getByLabel('密码', {exact: true}).fill(entryPassword);
  await vault.getByLabel('描述', {exact: true}).fill(description);
  await vault.getByRole('button', {name: '保存条目', exact: true}).click();
  await vault.locator('.vault-editor').waitFor({state:'detached'});
  await state('已解锁');
  const selectOwn=async()=>{await vault.getByLabel('搜索渠道或账号').fill(channel);await vault.locator('[data-entry-id]').filter({hasText:channel}).click();};
  await selectOwn();
  const row = () => vault.locator('.vault-details');
  await row().waitFor();
  const id = await vault.locator('[data-entry-id]').filter({hasText:channel}).getAttribute('data-entry-id');
  result.createdEntryId=id;result.createdChannel=channel;
  assert.equal(await row().locator('code').textContent(), '••••••••••••');
  assert.equal(await row().locator('.vault-description').textContent(), description);
  passed('native-description-create-exact-multiline');
  passed('native-add-entry-masked');
  await row().getByRole('button', {name: '显示密码', exact: true}).click();
  await until(async () => await row().locator('code').textContent() === entryPassword);
  await until(async () => await row().locator('code').textContent() === '••••••••••••', 18000);
  passed('real-15-second-reveal-expiry');
  await row().getByRole('button', {name: '复制账号', exact: true}).click();
  await vault.getByText('已复制账号', {exact: true}).waitFor();
  assert.ok(clipboard().replace(/\r\n/g, '\n') === account, 'Fictional account clipboard comparison failed');
  await row().getByRole('button', {name: '复制密码', exact: true}).click();
  await until(async () => clipboard() === entryPassword);
  passed('native-system-clipboard-account-and-password');
  await vault.getByRole('button', {name: '锁定密码库', exact: true}).click();
  await state('已锁定');
  assert.equal(await vault.locator('.vault-description').count(), 0);
  assert.ok(clipboard() === entryPassword, 'Fictional password clipboard comparison failed');
  await wrong('FICTIONAL-wrong-master');
  await unlock(master);
  await selectOwn();
  assert.equal(await vault.locator('[data-entry-id]').filter({hasText:channel}).getAttribute('data-entry-id'), id);
  assert.equal(await row().locator('.vault-description').textContent(), description);
  passed('description-hidden-while-locked-and-reloaded');
  passed('lock-wrong-master-reunlock-and-clipboard-retention');
  await row().getByRole('button', {name: '编辑', exact: true}).click();
  assert.equal(await vault.getByLabel('密码', {exact: true}).count(), 0);
  assert.equal(await vault.getByLabel('描述', {exact: true}).inputValue(), description);
  await vault.getByLabel('描述', {exact: true}).fill(editedDescription);
  await vault.getByLabel('账号', {exact: true}).fill(account + '\n已编辑');
  await vault.getByRole('button', {name: '保存条目', exact: true}).click();
  await vault.locator('.vault-editor').waitFor({state:'detached'});
  await state('已解锁');
  await row().waitFor();
  assert.equal(await row().locator('strong').textContent(), account + '\n已编辑');
  assert.equal(await row().locator('.vault-description').textContent(), editedDescription);
  passed('native-description-edit');
  passed('native-edit-entry');
  await row().getByRole('button', {name: '显示密码', exact: true}).click();
  await until(async () => await row().locator('code').textContent() === entryPassword);
  await row().getByRole('button', {name: '编辑', exact: true}).click();
  await vault.getByRole('button', {name: '修改密码', exact: true}).click();
  await vault.getByLabel('密码', {exact: true}).fill('FICTIONAL-cancelled-change');
  await vault.getByRole('button', {name: '取消修改密码', exact: true}).click();
  assert.equal(await vault.getByLabel('密码', {exact: true}).count(), 0);
  await vault.getByRole('button', {name: '保存条目', exact: true}).click();
  await vault.locator('.vault-editor').waitFor({state: 'detached'});
  await row().getByRole('button', {name: '显示密码', exact: true}).click();
  await until(async () => await row().locator('code').textContent() === entryPassword);
  await row().getByRole('button', {name: '编辑', exact: true}).click();
  await vault.getByRole('button', {name: '修改密码', exact: true}).click();
  await vault.getByRole('button', {name: '保存条目', exact: true}).click();
  assert.equal(await vault.getByLabel('密码', {exact: true}).evaluate(input => input.validity.valueMissing), true);
  await vault.getByLabel('密码', {exact: true}).fill('FICTIONAL-explicit-replacement');
  await vault.getByRole('button', {name: '保存条目', exact: true}).click();
  await vault.locator('.vault-editor').waitFor({state: 'detached'});
  await row().getByRole('button', {name: '显示密码', exact: true}).click();
  await until(async () => await row().locator('code').textContent() === 'FICTIONAL-explicit-replacement');
  // Restore this run's original fictional secret for later clipboard assertions.
  await row().getByRole('button', {name: '编辑', exact: true}).click();
  await vault.getByRole('button', {name: '修改密码', exact: true}).click();
  await vault.getByLabel('密码', {exact: true}).fill(entryPassword);
  await vault.getByRole('button', {name: '保存条目', exact: true}).click();
  await vault.locator('.vault-editor').waitFor({state: 'detached'});
  await until(async () => await vault.locator('.vault-message').textContent() === '', 5000);
  assert.equal(clipboard(), entryPassword);
  passed('metadata-secret-retention-cancel-required-replacement-and-short-notice');
  await vault.getByRole('button', {name: '更改主密码', exact: true}).click();
  const nextMaster = master === master1 ? master2 : master1;
  await vault.getByLabel('主密码', {exact: true}).fill(master);
  await vault.getByLabel('新主密码', {exact: true}).fill(nextMaster);
  await vault.getByLabel('再次输入主密码', {exact: true}).fill(nextMaster);
  assert.equal(await vault.getByLabel('主密码',{exact:true}).inputValue(),master);
  assert.equal(await vault.getByLabel('新主密码',{exact:true}).inputValue(),nextMaster);
  await vault.getByRole('button', {name: '保存主密码', exact: true}).click();
  await vault.getByText('主密码已更改。', {exact: true}).waitFor();
  await vault.getByRole('button', {name: '锁定密码库', exact: true}).click();
  await state('已锁定'); await wrong(master); await unlock(nextMaster);
  master = nextMaster;
  passed('change-master-old-rejected-new-accepted');
  if (process.env.SHIXU_NATIVE_SCREENSHOTS === '1') screenshotNative('desktop-combined-unlocked.png');
  await vault.screenshot({path: path.join(out, 'desktop-unlocked-masked.png'), fullPage: true});
  await row().getByRole('button',{name:'显示密码',exact:true}).click();
  await until(async () => await row().locator('code').textContent() !== '••••••••••••');
  await main.getByRole('button',{name:'通知',exact:true}).click();
  assert.equal(await main.evaluate(()=>window.__TAURI_INTERNALS__.invoke('get_vault_lock_state',{})),true);
  await main.getByRole('button',{name:'密码库',exact:true}).click();
  await state('已解锁');await selectOwn();assert.equal(await row().locator('code').textContent(),'••••••••••••');
  passed('module-switch-retains-session-and-redacts-password');
  for (let i = 0; i < 20; i++) {
    await row().getByRole('button', {name: '显示密码', exact: true}).click();
    // Intentionally switch immediately, without waiting for reveal to complete.
    await main.getByRole('button', {name: '通知', exact: true}).click();
    assert.equal(await main.evaluate(() => window.__TAURI_INTERNALS__.invoke('get_vault_lock_state', {})), true);
    await main.getByRole('button', {name: '密码库', exact: true}).click();
    await state('已解锁'); await selectOwn();
    assert.equal(await row().locator('code').textContent(), '••••••••••••');
  }
  passed('twenty-immediate-reveal-module-switches-retain-session');
  assert.equal(nativeWindow('count'),1,'one visible top-level native window');
  passed('single-native-window-with-two-webviews');
  nativeWindow('minimize');await sleep(500);nativeWindow('restore');await state('已解锁');
  passed('minimize-restore-retains-session');
  assert.equal(nativeWindow('close'),1);await sleep(600);assert.equal(nativeWindow('count'),0);
  assert.equal(await main.evaluate(()=>window.__TAURI_INTERNALS__.invoke('get_vault_lock_state',{})),true);
  nativeWindow('restore');await state('已解锁');await selectOwn();
  passed('native-close-to-tray-and-native-restore-retain-session');
  // Native restore does not substitute for manual tray menu acceptance.
  await main.getByRole('button',{name:'日历',exact:true}).click();
  await main.screenshot({path:path.join(out,'desktop-main-calendar.png')});
  await main.getByRole('button',{name:'密码库',exact:true}).click();await state('已解锁');
  nativeWindow('resize',760,700);await sleep(800);
  await checkHeaderAfterScroll();
  await vault.getByRole('button',{name:'锁定密码库',exact:true}).click();await state('已锁定');await unlock(master);await selectOwn();
  await row().getByRole('button',{name:'编辑',exact:true}).click();
  await checkHeaderAfterScroll();
  await vault.getByRole('button',{name:'取消',exact:true}).click();
  passed('narrow-window-scroll-lock-and-edit-pointer');
  assert.ok(await vault.getByRole('button',{name:'新增条目',exact:true}).isVisible());
  await vault.screenshot({path:path.join(out,'desktop-narrow-vault.png'),fullPage:true});
  nativeWindow('resize',1487,1058);await sleep(800);await vault.screenshot({path:path.join(out,'desktop-restored-vault.png')});
  await stopOwned();
  assert.ok(clipboard() === entryPassword, 'Fictional clipboard retention comparison failed');
  const ciphertext = fs.readFileSync(encrypted);
  for (const plaintext of [master, entryPassword, channel, account, editedDescription]) assert.equal(ciphertext.includes(Buffer.from(plaintext)), false);
  assert.equal(ciphertext.readUInt32LE(0), 0x9aa2d903);
  await launch(); await wrong(master === master1 ? master2 : master1); await unlock(master);
  await selectOwn();
  assert.equal(await vault.locator('[data-entry-id]').filter({hasText:channel}).getAttribute('data-entry-id'), id);
  assert.equal(await row().locator('strong').textContent(), account + '\n已编辑');
  assert.equal(await row().locator('.vault-description').textContent(), editedDescription);
  passed('description-encrypted-and-persists-after-process-restart');
  passed('actual-process-restart-encrypted-file-reopened');
  await row().getByRole('button', {name: '编辑', exact: true}).click();
  await vault.getByLabel('描述', {exact: true}).fill('');
  await vault.getByRole('button', {name: '保存条目', exact: true}).click();
  await vault.locator('.vault-editor').waitFor({state:'detached'});
  await state('已解锁');
  await row().waitFor();
  assert.equal(await row().locator('.vault-description').textContent(), '暂无描述');
  await vault.getByRole('button', {name: '锁定密码库', exact: true}).click();
  await state('已锁定'); await unlock(master);
  await selectOwn();
  assert.equal(await row().locator('.vault-description').textContent(), '暂无描述');
  passed('native-description-clear-persists-after-reunlock');
  // Delete only the uniquely named entry created by this run, after persistence proof.
  await row().getByRole('button', {name: '删除条目', exact: true}).click();
  await vault.getByRole('button', {name: '确认删除', exact: true}).click();
  await vault.locator(`[data-entry-id="${id}"]`).waitFor({state: 'detached'});
  passed('native-delete-own-fictional-entry');
  console.log('WAIT real 310-second idle; no injected clock or vault activity');
  const idleStart=Date.now();
  await sleep(310000);
  result.idleElapsedMs=Date.now()-idleStart;
  await state('已解锁');
  const idleRows=await vault.evaluate(()=>window.__TAURI_INTERNALS__.invoke('vault_list',{}));
  assert.ok(Array.isArray(idleRows));assert.ok(result.idleElapsedMs>=310000);
  await vault.screenshot({path:path.join(out,'desktop-idle-unlocked.png'),fullPage:true});
  passed('real-310-second-native-and-UI-session-retained');
  await vault.getByRole('button',{name:'锁定密码库',exact:true}).click();await state('已锁定');
  await vault.screenshot({path:path.join(out,'desktop-inline-locked.png'),fullPage:true});
  if (process.env.SHIXU_NATIVE_SCREENSHOTS === '1') screenshotNative('desktop-combined-locked.png');
  await unlock(master);
  assert.equal(await vault.locator(`[data-entry-id="${id}"]`).count(), 0);
  passed('deleted-entry-absent-after-reunlock');
  await vault.getByRole('button', {name: '锁定密码库', exact: true}).click();
  await state('已锁定');
  result.fictionalMasterFixture = master === master1 ? 'A' : 'B';
  result.complete = true;
})().catch(async error => {
  result.complete = false; result.error = String(error.stack || error);
  console.error(result.error); process.exitCode = 1;
  if(vault && !vault.isClosed()) {
    result.ui = {state: await vault.locator('.vault-state').textContent().catch(()=>''),
      message: await vault.locator('.vault-message').textContent().catch(()=>'')};
    await vault.screenshot({path: path.join(out, 'desktop-failure.png'), timeout: 5000}).catch(()=>{});
  }
}).finally(async () => {
  await stopOwned();
  fs.writeFileSync(path.join(out, 'desktop-result.json'), JSON.stringify(result, null, 2));
});
