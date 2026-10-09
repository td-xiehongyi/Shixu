# F0 toolchain and dependency provenance

Verified on 2026-10-09 in Linux x86_64 (`x86_64-unknown-linux-gnu`). This is a domain-contract baseline; no Windows application, QQ component, password engine or attachment parser is implemented or verified.

## Reproduction

Rust is installed in workspace-local directories by the controller using official rustup. The toolchain and components are pinned in `rust-toolchain.toml`; ordinary dependency installation was explicitly authorized.

```bash
export RUSTUP_HOME=/workspace/.toolchains/rustup
export CARGO_HOME=/workspace/.toolchains/cargo
export PATH=/workspace/.toolchains/cargo/bin:$PATH
cd /workspace/shixu-v0.1
cargo test -p shixu-core --test contract_roundtrip
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy -p shixu-core --all-targets -- -D warnings
```

These directory variables contain toolchain locations only, never credentials. On another machine, use the same pinned toolchain with that machine's standard Cargo/Rustup directories and repository root.

| Tool | Actual output |
| --- | --- |
| rustc | `rustc 1.99.0 (b940084d7 2026-09-28)` |
| Cargo | `cargo 1.99.0 (5f94df478 2026-08-27)` |
| rustfmt | `rustfmt 1.10.0-stable (b940084d7e 2026-09-28)` |
| clippy | `clippy 0.1.99 (b940084d7e 2026-09-28)` |

[Official Rust installation](https://www.rust-lang.org/tools/install) and [rustup toolchain pinning](https://rust-lang.github.io/rustup/overrides.html#the-toolchain-file) describe the installation/pinning mechanism. The first version-specific invocation downloaded the named 1.99.0 toolchain and rustfmt/clippy components after the controller's stable installation; no administrator escalation was needed.

## Dependencies

Direct dependency declarations use exact versions: serde 1.0.229 (derive), uuid 1.27.0 (serde, no UUID-generation/randomness feature), zeroize 1.9.1, and test-only serde_json 1.0.151. These implement typed DTO serialization, UUID validation, and secret-memory clearing; none implements password encryption, QQ, networking, Tauri or parsing attachments.

`Cargo.lock` is committed and records exact transitive versions, registry sources and package checksums. All external packages below were resolved from the crates.io registry; table metadata comes from `cargo metadata --locked --format-version 1`. Target-specific packages can appear in the lockfile without being compiled for Linux. This provenance record is not a security audit or permission to redistribute a future third-party application.

| Package | Locked version | Declared license | Upstream repository |
| --- | --- | --- | --- |
| bumpalo | 3.20.3 | MIT OR Apache-2.0 | [bumpalo](https://github.com/fitzgen/bumpalo) |
| cfg-if | 1.0.5 | MIT OR Apache-2.0 | [cfg-if](https://github.com/rust-lang/cfg-if) |
| futures-core | 0.3.34 | MIT OR Apache-2.0 | [futures-core](https://github.com/rust-lang/futures-rs) |
| futures-task | 0.3.34 | MIT OR Apache-2.0 | [futures-task](https://github.com/rust-lang/futures-rs) |
| futures-util | 0.3.34 | MIT OR Apache-2.0 | [futures-util](https://github.com/rust-lang/futures-rs) |
| itoa | 1.0.18 | MIT OR Apache-2.0 | [itoa](https://github.com/dtolnay/itoa) |
| js-sys | 0.3.106 | MIT OR Apache-2.0 | [js-sys](https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/js-sys) |
| memchr | 2.8.3 | Unlicense OR MIT | [memchr](https://github.com/BurntSushi/memchr) |
| once_cell | 1.21.4 | MIT OR Apache-2.0 | [once_cell](https://github.com/matklad/once_cell) |
| pin-project-lite | 0.2.17 | Apache-2.0 OR MIT | [pin-project-lite](https://github.com/taiki-e/pin-project-lite) |
| proc-macro2 | 1.0.107 | MIT OR Apache-2.0 | [proc-macro2](https://github.com/dtolnay/proc-macro2) |
| quote | 1.0.47 | MIT OR Apache-2.0 | [quote](https://github.com/dtolnay/quote) |
| rustversion | 1.0.23 | MIT OR Apache-2.0 | [rustversion](https://github.com/dtolnay/rustversion) |
| serde | 1.0.229 | MIT OR Apache-2.0 | [serde](https://github.com/serde-rs/serde) |
| serde_core | 1.0.229 | MIT OR Apache-2.0 | [serde_core](https://github.com/serde-rs/serde) |
| serde_derive | 1.0.229 | MIT OR Apache-2.0 | [serde_derive](https://github.com/serde-rs/serde) |
| serde_json | 1.0.151 | MIT OR Apache-2.0 | [serde_json](https://github.com/serde-rs/json) |
| slab | 0.4.12 | MIT | [slab](https://github.com/tokio-rs/slab) |
| syn | 3.0.6 | MIT OR Apache-2.0 | [syn](https://github.com/dtolnay/syn) |
| unicode-ident | 1.0.26 | (MIT OR Apache-2.0) AND Unicode-3.0 | [unicode-ident](https://github.com/dtolnay/unicode-ident) |
| uuid | 1.27.0 | Apache-2.0 OR MIT | [uuid](https://github.com/uuid-rs/uuid) |
| wasm-bindgen | 0.2.129 | MIT OR Apache-2.0 | [wasm-bindgen](https://github.com/wasm-bindgen/wasm-bindgen) |
| wasm-bindgen-macro | 0.2.129 | MIT OR Apache-2.0 | [wasm-bindgen-macro](https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/macro) |
| wasm-bindgen-macro-support | 0.2.129 | MIT OR Apache-2.0 | [wasm-bindgen-macro-support](https://github.com/wasm-bindgen/wasm-bindgen/tree/main/crates/macro-support) |
| wasm-bindgen-shared | 0.2.129 | MIT OR Apache-2.0 | [wasm-bindgen-shared](https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/shared) |
| zeroize | 1.9.1 | Apache-2.0 OR MIT | [zeroize](https://github.com/RustCrypto/utils) |
| zmij | 1.0.23 | MIT | [zmij](https://github.com/dtolnay/zmij) |

## Contract choices and verification

- Public IDs are distinct UUID-backed string newtypes. `Revision = u64`, `UtcMillis = i64`. Wire fields and enum values use snake_case; `AppError` uses fixed uppercase IPC codes and no error payload.
- Backup/restore schema version 1 is the only accepted version. Missing/invalid/unknown schema versions fail decoding. There is no hidden default to interpret an unknown version.
- `CalendarEvent` keeps the design's flat `time_precision`, date/time and timezone fields. Reusable `TimeValue` uses `precision`; missing dates and clock times stay null. This baseline does not perform date interpretation or semantic timezone validation.
- Optional location patches distinguish leaving a field unchanged (`null`/absent) from `{"operation":"set","value":"..."}` and `{"operation":"clear"}`. Writes carry the required expected revision. Revision enforcement is a future service responsibility.
- `SecretBytes` owns `zeroize::Zeroizing<Vec<u8>>`, provides explicit borrowed exposure and clearing, and omits Debug, Clone and serde. VaultRecord/VaultMutation omit Debug and serde; VaultSummary has no password field. SessionId stays internal and is not serialized. The wrapper clears on drop via the pinned dependency; tests deliberately do not dereference freed memory. [zeroize's upstream notes](https://docs.rs/zeroize/1.9.1/zeroize/) explain the limits of memory clearing, including earlier copies, reallocation, registers and swap.
- Persistent attachment references are native identifiers, not URLs or disk paths. The string newtype rejects empty strings, slash/backslash/colon and control characters. Adapters must map their native references to this representation; transient download URLs and credentials have no DTO field.
- Fixture helpers derive a read-only root from the crate manifest directory, require a version-1 manifest with explicit synthetic provenance, validate the entire allowlist, reject traversal/absolute/Windows paths and symlinks, and cap manifest/fixture reads at 64 KiB/50 MiB. The only tracked fixture is the reviewed synthetic event JSON; byte loading is the future attachment-fixture entry point. Loader restriction tests create and remove disposable synthetic files under the system temporary directory. No personal directory or real secret is an input.
- ParserLimits records every specified parser/download resource default. It is not OS-level enforcement. Backup retention, lock timers, engine integration and actual parser execution remain in their subsequent tasks.
- RestorePreview is data tied to a preview ID, not a restore command or authorization. D6 must implement a separate confirmation action before overwrite.

Final F0 results: **22/22 contract integration tests and 5/5 compile-fail doctests pass**. Both workspace crates build in the full suite; the native crate has no integration implementation/tests yet. Formatting and strict clippy passed. No tests were ignored. The contract suite requires no QQ or KeePassXC installation, no `.env`, and no credentials.

Implementation evidence, red/green runs and self-review notes are in the controller's ignored `.superpowers/sdd/shixu-v0.1/task-F0-report.md`. Windows/QQ/password-engine gates remain unexecuted and must not be inferred from this Linux result.

## V1 dependency update

The F0 dependency table above is historical baseline evidence. V1 enabled `uuid/v4` for session ID generation; the current `Cargo.toml` and `Cargo.lock` are authoritative for the present dependency graph. Versions, licenses and upstream repositories for the V1 randomness dependency set are:

| Package | Version | Declared license | Upstream repository |
| --- | --- | --- | --- |
| uuid | 1.27.0 | Apache-2.0 OR MIT | [uuid](https://github.com/uuid-rs/uuid) |
| getrandom | 0.4.3 | MIT OR Apache-2.0 | [getrandom](https://github.com/rust-random/getrandom) |
| libc | 0.2.190 | MIT OR Apache-2.0 | [libc](https://github.com/rust-lang/libc) |
| r-efi | 6.0.0 | MIT OR Apache-2.0 OR LGPL-2.1-or-later | [r-efi](https://github.com/r-efi/r-efi) |

These packages resolved from `registry+https://github.com/rust-lang/crates.io-index`. `r-efi` is a target-specific transitive dependency; this Linux verification does not test a Windows or EFI backend. This provenance note records declared metadata and does not imply a security audit.

## N1 persistence and Windows protection update

N1 adds exactly pinned direct `rusqlite = 0.40.2` (default features disabled, `bundled` only), `sha2 = 0.11.0`, and Windows-target-only `windows-sys = 0.61.2`. `serde_json = 1.0.151` now also serializes protected runtime DTO payloads. No test protector is exported by either product crate: synthetic reversible doubles exist only inside integration test binaries. SHA-256 is identity/content hashing, never product encryption.

The locked `libsqlite3-sys = 0.38.2` bundles SQLite **3.53.2**, as read from the registry source's `sqlite3.h`; SQLite's upstream code is [public domain](https://www.sqlite.org/copyright.html). All new registry packages below come from crates.io; `Cargo.lock` records sources and checksums. Table entries are actual `cargo metadata --locked --format-version 1` declared license/repository metadata, not a security audit.

| Package | Version | Declared license | Upstream repository |
| --- | --- | --- | --- |
| bitflags | 2.13.2 | MIT OR Apache-2.0 | https://github.com/bitflags/bitflags |
| block-buffer | 0.12.1 | MIT OR Apache-2.0 | https://github.com/RustCrypto/utils |
| cc | 1.6.0 | MIT OR Apache-2.0 | https://github.com/rust-lang/cc-rs |
| const-oid | 0.10.2 | Apache-2.0 OR MIT | https://github.com/RustCrypto/formats |
| cpufeatures | 0.3.1 | MIT OR Apache-2.0 | https://github.com/RustCrypto/utils |
| crypto-common | 0.2.2 | MIT OR Apache-2.0 | https://github.com/RustCrypto/traits |
| digest | 0.11.3 | MIT OR Apache-2.0 | https://github.com/RustCrypto/traits |
| fallible-iterator | 0.3.0 | MIT/Apache-2.0 | https://github.com/sfackler/rust-fallible-iterator |
| fallible-streaming-iterator | 0.1.9 | MIT/Apache-2.0 | https://github.com/sfackler/fallible-streaming-iterator |
| find-msvc-tools | 0.1.14 | MIT OR Apache-2.0 | https://github.com/rust-lang/cc-rs |
| hybrid-array | 0.4.15 | MIT OR Apache-2.0 | https://github.com/RustCrypto/hybrid-array |
| libsqlite3-sys | 0.38.2 | MIT | https://github.com/rusqlite/rusqlite |
| pkg-config | 0.3.34 | MIT OR Apache-2.0 | https://github.com/rust-lang/pkg-config-rs |
| rusqlite | 0.40.2 | MIT | https://github.com/rusqlite/rusqlite |
| sha2 | 0.11.0 | MIT OR Apache-2.0 | https://github.com/RustCrypto/hashes |
| shlex | 2.0.1 | MIT OR Apache-2.0 | https://github.com/comex/rust-shlex |
| smallvec | 1.16.2 | MIT OR Apache-2.0 | https://github.com/servo/rust-smallvec |
| typenum | 1.20.1 | MIT OR Apache-2.0 | https://github.com/paholg/typenum |
| vcpkg | 0.2.15 | MIT/Apache-2.0 | https://github.com/mcgoo/vcpkg-rs |
| windows-link | 0.2.1 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-sys | 0.61.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |

Core/native APIs preserve F0 DTO fields and wire names; the sole required contract addition is explicit `SourceCapability::Edits` (`edits`). Changes without this declared adapter capability return `RevisionConflict`. SQLite stores protected message/sender/filename/evidence payloads and protected content digests; technical identity/date/state columns remain indexable. Current-user DPAPI uses `CRYPTPROTECT_UI_FORBIDDEN`, app-specific constant entropy and no machine-scope flag. Its small Win32 boundary is the only module allowing unsafe code; buffer/handle ownership and Win32 release operations are documented beside each block. Native `open_database` applies a current-user-only inheritable protected ACL to an app-owned leaf before SQLite creation, and reapplies it to existing database/WAL/SHM files. It rejects leaf/file reparse points. The caller must provide a trusted app-owned directory and parent; ancestor replacement/race resistance still requires Windows review.

API behavior is based on primary documentation: [CryptProtectData](https://learn.microsoft.com/en-us/windows/win32/api/dpapi/nf-dpapi-cryptprotectdata), [CryptUnprotectData](https://learn.microsoft.com/en-us/windows/win32/api/dpapi/nf-dpapi-cryptunprotectdata), [CreateDirectoryW](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createdirectoryw), [SetFileSecurityW](https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-setfilesecurityw), and [security descriptor conversion/allocation](https://learn.microsoft.com/en-us/windows/win32/api/sddl/nf-sddl-convertstringsecuritydescriptortosecuritydescriptorw). These do not establish actual Windows behavior in this Linux run. DPAPI restoration across machines is not promised.

N1 final Linux evidence: 16/16 persistence integration tests; workspace 23 contract + 16 message + 31 vault + 1 unsupported-native + 7 compile-fail doctests pass; two real Windows tests remain explicitly ignored in the ordinary suite. `cargo fmt --all -- --check` and workspace strict clippy pass. The persistence test actually kills a writer process after commit and reopens the SQLite database to assert the durable message remains pending.

The explicit native command `cargo test -p shixu-native --test data_protection -- --ignored` returns exit 101 with both tests reporting **BLOCKED**, because this host lacks Windows DPAPI/ACL and a second real Windows identity. This is not native acceptance. A normal `cargo check -p shixu-native --all-targets --locked --target x86_64-pc-windows-gnu` also returns exit 101: bundled SQLite cannot compile without `x86_64-w64-mingw32-gcc`. No Windows C compiler or administrator package was installed.

For **Rust Windows cfg/type checking only**, the official rustup Windows GNU standard-library target was installed. `LIBSQLITE3_SYS_USE_PKG_CONFIG=1 cargo check -p shixu-native --all-targets --locked --target x86_64-pc-windows-gnu` and the matching strict-clippy command pass. This environment option switches libsqlite3-sys away from its bundled C build to declared external-library bindings; pkg-config does not find an actual Windows SQLite library. Cargo check/clippy type-check Rust without final linking, so these results prove Win32 Rust API/cfg compatibility only. They do not prove a Windows application build, SQLite target library, DPAPI operation, user ACL, other-identity denial or cross-machine restoration. The unmodified default manifest still uses bundled SQLite for product builds. Raw success and blocker logs are retained in ignored `.superpowers/sdd/shixu-v0.1/task-N1-*.log`.

## D2 desktop foundation (2026-10-09)

D2 adds the React/TypeScript weekly shell, restricted native command dispatch, and a Windows-only Tauri member. This is a foundation milestone: it does not establish a Windows executable, actual WebView IPC denial, a verified password engine, QQ connectivity, or finished D3/D4 interactions.

The actual environment is Node **24.19.0**, npm **11.9.0**, pnpm **11.19.0** and the unchanged Rust **1.99.0** toolchain above. `packageManager` pins pnpm; every direct npm/Cargo dependency has an exact version, and both lockfiles preserve transitive versions and integrity/checksums. `pnpm-workspace.yaml` disables automatic installation while executing checks; installation remains an explicit operation. Its version-specific release-age exceptions were generated by pnpm for the explicitly selected packages.

| Direct dependency | Pin | Declared license | Upstream |
| --- | --- | --- | --- |
| React / React DOM | 19.3.0 | MIT | https://github.com/react/react |
| TypeScript | 7.0.2 | Apache-2.0 | https://github.com/microsoft/TypeScript |
| Vite | 8.3.4 | MIT | https://github.com/vitejs/vite |
| Vitest | 5.0.3 | MIT | https://github.com/vitest-dev/vitest |
| Vite React plugin | 6.1.2 | MIT | https://github.com/vitejs/vite-plugin-react |
| React / React DOM types | 19.3.0 | MIT | https://github.com/DefinitelyTyped/DefinitelyTyped |
| Node types | 26.6.4 | MIT | https://github.com/DefinitelyTyped/DefinitelyTyped |
| Tauri JS API / CLI | 2.12.2 / 2.12.1 | Apache-2.0 OR MIT | https://github.com/tauri-apps/tauri |
| Tauri / tauri-build | 2.12.2 / 2.7.1 | Apache-2.0 OR MIT | https://github.com/tauri-apps/tauri |
| Phosphor React icons | 2.1.10 | MIT | https://github.com/phosphor-icons/react |
| Fontsource variable Noto Sans SC | 5.3.0 | OFL-1.1 | https://github.com/fontsource/font-files |
| Prettier | 3.9.9 | MIT | https://github.com/prettier/prettier |

Actual installed npm metadata (52 packages) is recorded in [d2-npm-provenance.json](d2-npm-provenance.json). All 341 newly locked external Cargo package metadata records, including target-specific dependencies, are recorded in [d2-cargo-provenance.json](d2-cargo-provenance.json), from `cargo metadata --locked --format-version 1`. Every new Cargo record has declared license or license-file metadata. No pre-D2 locked package/version was removed. These records are provenance and declared licensing, not a security audit. Platform packages in the lockfile do not imply that their runtimes were built or installed. Font files are bundled locally through the npm package; the app requests no remote font/image resources.

Security choices follow [Tauri capabilities](https://v2.tauri.app/security/capabilities/), [permissions](https://v2.tauri.app/security/permissions/), and the pinned Tauri `WebviewWindowBuilder` source/documentation. The app manifest includes every custom command so capabilities also apply to application commands. Main and vault capabilities grant disjoint command lists, no remote URLs, and no shell/filesystem plugin ports. Native dispatch separately checks the injected calling WebView label and its actual URL origin. Navigation only admits bundled index pages. Window creation runs outside the synchronous Windows WebView callback. CSP excludes remote scripts/frames/objects. The [Phosphor icon family](https://phosphoricons.com/) supplies UI glyphs; [Fontsource's Noto Sans SC license](https://fontsource.org/fonts/noto-sans-sc/about) identifies the CJK font as OFL-1.1. The exact original reference font is unknown; no identification claim is made.

Native wire DTOs convert core `u64` revisions/counts to validated decimal strings **before JSON serialization**. Inbound patch/undo/vault revisions reject noncanonical or out-of-range decimals. The common Rust/TS synthetic event fixture contains revision `9007199254740993`; Rust serialization and TypeScript reception preserve it. Milliseconds outside the JavaScript safe integer range are rejected separately. Main bridge exposes no vault method or session ID. Valid vault calls remain `UNSUPPORTED` until V2; there is no plaintext/fake product protection fallback on Linux.

Portable reproduction:

```bash
pnpm install --frozen-lockfile
pnpm test
pnpm run typecheck
pnpm run build
pnpm run format:check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
```

This workspace's package-manager store was selected with `pnpm install --store-dir /tmp/shixu-pnpm-store`; this is an environment-local location, not a product requirement. The Rust exports documented above remain necessary here. Tauri runtime/build dependencies are Windows-target-only, so normal Linux workspace checks compile and run the portable policy/wire tests without GTK/WebKit packages. On a properly provisioned Windows machine, build the frontend before `pnpm tauri dev`; `pnpm tauri build` uses the declared `custom-protocol` feature. Installer bundling is disabled at this foundation stage. Real native acceptance belongs to D7.

D2 evidence: **8/8 frontend contract tests**, **291/291 Rust workspace tests**, **0 failures**, **3 existing Windows gates ignored**, strict Clippy/format/typecheck/build pass. The seven desktop tests are portable policy/wire/dispatch contracts; they are **not actual WebView IPC tests**. The original `cargo check -p shixu-desktop --target x86_64-pc-windows-gnu --locked` attempt failed at bundled SQLite because `x86_64-w64-mingw32-gcc` is absent. It did not compile/validate the Windows adapter. No system compiler or GTK package was installed.

Installed Playwright and `/usr/bin/chromium` rendered the actual production frontend at 1487×1058 and 390×844. Calendar week/today controls, principal navigation, disconnected default, explicit demo isolation, vault entry feedback, and narrow internal scrolling were exercised with **0 console/page errors**. Screenshots and exact logs are in ignored `.superpowers/sdd/shixu-v0.1/D2-*`. The controller directly observed the selected reference and reviewed rendered captures; no combined source/output artifact is available because the original has no local path. Browser rendering does not prove native WebView, Windows security/runtime, or complete D3/D4 feature acceptance. No site was deployed.
