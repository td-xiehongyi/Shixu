# Windows minimum desktop and vault lifecycle

Stage C implements notification wiring and engineering build inputs. Windows production vault remains `Unsupported`; Q1 stays OPEN and all twelve release gates stay BLOCKED. Cloud type checks and injected tests do not accept Windows behavior or enable production.

Use an installed standard-user x64 Visual Studio Developer PowerShell with C++ compiler, linker and Windows SDK resource compiler; installed Rust 1.99.0 MSVC host/target and locked offline Cargo cache; Python; frontend Node24.19.0/pnpm11.19.0 with frozen dependencies already prepared; and installed WebView2 for observation. The scripts install none of these prerequisites. Missing components produce a specific BLOCKED result. Prepare prerequisites separately; no credentials, QQ login or production KDBX are needed.

Explicit public runtime preparation remains:

```powershell
python scripts/prepare-vault-runtime.py --platform win-x64
python scripts/prepare-vault-runtime.py --platform win-x64 --verify-only
```

This fixed pinned preparation fills the user-temp archive/npm cache and verifies the 198-file reviewed manifest. It is never run at startup. The minimum workflow uses the same preparation with `--offline`, verifies the engineering ICO, builds the frontend, runs the exact Windows trusted-path constructor regression, invokes the existing actual synthetic native boundary runner (exact one parent test plus production gate), performs the real MSVC desktop build with custom protocol, and runs three exact synthetic controller regressions (blocked reveal, serialized reply and blocked startup):

```powershell
.\scripts\test-windows-vault-minimum.ps1
```

Each direct external exit and safe full output is recorded under a unique ignored evidence directory. The final result includes committed source hashes, executable hash and all 198 copied resource hashes. Native boundary evidence retains its separate exact test/case counts. Build success is not installer or full release acceptance. Bundle remains inactive. The ICO is a deterministic engineering asset, not final branding; regenerate/check with the two checked-in Python icon scripts.

For interactive observation, use the fixed built unavailable-vault desktop:

```powershell
.\scripts\observe-windows-vault-lifecycle.ps1
```

The script accepts no arguments and launches only `--vault-lifecycle-observe`, recording that mode and the pre-launch executable hash. Startup atomically creates a fresh process-owned synthetic temp root; it never opens normal application data, DPAPI calendar/config, QQ controller or queued workers. Both main and vault WebView profiles use the synthetic root rather than the normal application profile. The unavailable controller still receives the same native lifecycle hooks. Synthetic browser files may remain in that temp root after exit; cleanup refuses recursive deletion.

Open the dedicated vault window, leave it locked, close main to tray, manually lock/unlock Windows, then manually suspend/resume through Windows UI. Exit through the tray. The script never forces a lock/suspend or accepts credentials, vault paths, programs or capabilities. Review `SHIXU_VAULT_LIFECYCLE` lines alongside a manual action record: registration readiness, deferred revocation, actual session lock/unlock, suspend/automatic resume and window-close events. Pending events can coalesce; these are dispatcher receipt logs, not callback latency or production-engine proof. A SendMessage test alone is not proof that Windows emitted the event. Production stays unavailable during this observation.

Actual hooks register the retained top-level main HWND on its owning UI thread with `SetWindowSubclass` and this-session WTS registration. The callback only updates same-controller atomic generation/block/pending state and tries a capacity-one wake. A full wake queue retains event bits; disconnected/panicking consumer fails closed. Unhandled messages forward through Tauri's subclass chain. Registration failure blocks optional vault availability. Exit/WM_NCDESTROY unregister and remove on the owning thread, with failure retained as blocked availability. Calendar/QQ availability is independent of the optional vault controller.

A bounded dispatcher emits fixed empty `vault_locked` only to the trusted vault WebView before waiting for authority/cancellation cleanup, then handles supervisor power transitions. Unlock/resume invalidates existing sessions and allows only a fresh manual unlock when all reasons permit it; duplicate automatic resume does not restart twice. Main close-to-tray and vault close invalidate through this same owner. This stage adds no focus-loss policy.

Native requests, worker acceptance and pending replies retain their original lifecycle generation, authority epoch and internal service session. The clipboard writer rechecks the native predicate after buffers and OpenClipboard are prepared, immediately before EmptyClipboard. Actual Tauri delivery rechecks after IpcResponse serialization immediately before consuming the resolver. The UI admits authentication and secret actions only after its owned native subscription succeeds. Pending or failed registration keeps controls unavailable; reopening permits a deliberate new subscription. The UI subscribes only to native lock events, clears owned input/reveal bytes and rows, and retires listeners on StrictMode/unmount/late registration. Frontend event emission has no granted capability.

A publication already linearized before notification cannot be retroactively withdrawn from WebView or clipboard. Atomic checks do not make OS/IPC delivery transactional. Physical helper termination/wiping waits for deferred cleanup; immutable JS/serialization/library copies and OS memory cannot be guaranteed erased. Fifteen-second reveal masking, five-minute idle and persistent clipboard semantics remain. Clipboard is never automatically cleared or rolled back.

Trusted native resource, Node and helper paths now join individual components; the strict policy still rejects slash-containing external paths. The constructor regression is Windows-only and must run on Windows.

Remaining native proof includes real WTS/power receipt and callback timing under blocked work, Tauri/WebView subclass coexistence, MSVC resource/link success, Job/AppContainer/token/ACL/reparse/network/handle/storage protections, native clipboard history/cloud and OS-damage boundaries. The unavailable desktop observation cannot establish a real-engine publication race; a separate authorized native synthetic controller probe must do that before production enablement. No gate is upgraded by this workflow.
