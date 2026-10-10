# KdbxWeb independent backend stage

This is an internal native development backend, not a released password feature.
UI commands remain disconnected; `bundle.active` remains false. Windows
`KdbxWebEngine::prepared` returns `Unsupported` until Job/restricted-token,
resource/network isolation, ACL/reparse and native file replacement gates have
real Windows evidence. Node permission is a trusted-code seat belt, not an OS
malicious-code sandbox. G1/Q1/OPEN12/release remain BLOCKED.

The native API implements all `VaultEngine` methods. Trusted native configuration
passes an absolute prepared resource root and a dedicated vault work directory;
there is no Tauri command accepting a program, raw command or arbitrary path.
The work directory is separate from calendar/message data and mode0700 on Linux.
Only the independent helper retains KdbxCredentials and decrypted database.
Rust owns no cached master. Ordinary operations never re-send unlock material.
`cancellation()` returns a cloneable native `VaultCancellation` for an already
started helper, usable from another thread during blocked IO. It is None before
create/open starts: next UI actor must establish pending-unlock cancellation
registration, immediately revoke its session/epoch and guard late delivery.

Sensitive input travels only over private inherited stdin/stdout, with big-endian
length-prefixed JSON, protocolv1, monotonically checked request IDs, fixed typed
operations and response variants. Access is serial (`&mut self`), with one
bounded worker queue/in-flight request. Frames<=1MiB; timeout30s covers write and
read, errors terminate/wait/join helper and leave engine closed. A native
cancellation handle kills without taking the IO ownership lock. No raw-command
variant, shell invocation, plaintext temp, secret argv/env, diagnostic output,
inspector or runtime installation exists. Child environment is entirely cleared,
including NODE_OPTIONS/NODE_PATH and search-path overrides.

Rust input/frame/selected-secret buffers zeroize on drop; helper mutable byte
buffers wipe best-effort. JSON strings, ProtectedValue/library/JS/WASM copies,
allocator copies, swap and same-user malicious processes cannot be claimed erased.
List returns internal `VaultSummary` (account but no password), reveal returns one
selected password. Accounts must remain confined to the vault capability during
future UI wiring; ordinary main DTOs must not gain them.

Supported stage format: Shixu's flat three-field KDBX4, AES, **uncompressed**, with
Argon2id version19. Created costs:64MiB,3 iterations,1 lane. Accepted costs:8KiB–
64MiB,1–4 iterations,1 lane,32-byte salt/output; exact UInt32/UInt64/Bytes metadata
kinds, unknown/duplicate keys/K/A/older version/other KDF rejected before KDF.
Compressed input returns Unsupported before any gzip expansion. Files<=8MiB,
header<=64KiB,field<=64KiB,entries<=1000,Node old-space<=128MiB. These are engineering
limits, not Windows performance/security validation or a total native-memory cap.
Only valid UTF-8 text is accepted by the real backend. Master/account/password
reject CR/LF/NEL/LS/PS; no trim or replacement. Title/UserName/Password map to
standard protected fields; UUIDs use standard RFC UUID generation. Channel
newlines including CR are preserved through protected-value encoding. XML parser
rejects DTD/entity declarations and every parser error, with no recovery.

Helper saves encrypted `vault.pending.kdbx`, reloads and compares UUID/revision/
all three fields/timestamps, then sends its ciphertext SHA256. Native verifies
that digest, fsyncs pending, checks expected active digest, fsyncs an encrypted
previous checkpoint and atomically renames on Linux, then fsyncs directory before
returning success. Node26 permission deliberately disables fsync APIs, so the
native committer performs durability barriers. Conflict/storage/interruption
never returns saved success. Same-user races between final digest check and
rename are outside this trusted single-owner API; production OS gate is pending.
Linux process/file checks are not proof of power-loss durability or Windows
semantics. A failure after replace may leave new valid current+old checkpoint;
full recovery selection UI is still blocked. This checkpoint is **not** the
planned ten-backup/restore system.

Explicit preparation: `python3 scripts/prepare-vault-runtime.py` downloads fixed
Node26.11.1 Linux/Windows x64 official archives, verifies pinned SHA256, runs
separately locked npm ci with scripts disabled, builds published KdbxWeb2.1.1 TS
source using TypeScript5.9.3, then compares every prepared file against reviewed
manifests. Prepared binary trees are ignored. Runtime never runs this script or
falls back to system Node. Native embeds the Linux manifest and rejects missing,
extra, symlinked or hash-altered assets. Windows manifest is packaging-only until
Windows gate implementation. `--update-reviewed-manifest` is an explicit maintainer
operation for reviewed source changes, never startup behavior.

The published KdbxWeb dist bundles old fflate; it is not the runtime entrypoint.
Rebuilt source actually resolves locked fflate0.8.3 and xmldom0.9.12. A strict
DOMParser compatibility wrapper replaces the removed legacy errorHandler API.
The complete official Node LICENSE (including third-party notices) ships beside
the binary. KdbxWeb/hash-wasm/xmldom/fflate MIT licenses ship in the prepared helper.
Compiler5.9.3 Apache2 is a preparation-only dependency; package integrity/licensing
is in the separate lock. npm audit evidence is advisory database evidence, not
absence-of-vulnerabilities proof. Independent decoder tests use the untouched
published upstream bundle, never the application helper API.

Sources: [Node26 permission documentation](https://nodejs.org/docs/latest-v26.x/api/permissions.html),
[official Node26.11.1 checksums](https://nodejs.org/dist/v26.11.1/SHASUMS256.txt),
[KdbxWeb upstream](https://github.com/keeweb/kdbxweb),
[hash-wasm upstream](https://github.com/Daninet/hash-wasm).
