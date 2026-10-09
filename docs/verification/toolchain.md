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
