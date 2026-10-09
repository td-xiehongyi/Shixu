# N3 parser selection and acceptance gates

Recorded 2026-10-09. **Portable subsystem implemented; Windows isolation and real format acceptance BLOCKED.** Product `host::parse_part` returns `Unsupported` on every OS. The reserved `shixu-parser` binary exits 78 (`UNSUPPORTED`) without opening an input or spawning a program. This is a safety boundary, not a sandbox implementation.

## Candidate provenance

| Candidate | Fixed evaluation target / source | License evidence | Current evidence |
|---|---|---|---|
| Tesseract OCR | [5.5.3 official release](https://github.com/tesseract-ocr/tesseract/releases/tag/5.5.3) | [Apache-2.0](https://raw.githubusercontent.com/tesseract-ocr/tesseract/5.5.3/LICENSE) | Source candidate only. No native executable acquired/built, no OCR/512 MiB/30 second acceptance. |
| Chinese OCR model | [`tessdata_fast` commit 87416418657359cb625c412a48b6e1d6d41c29bd, chi_sim.traineddata](https://raw.githubusercontent.com/tesseract-ocr/tessdata_fast/87416418657359cb625c412a48b6e1d6d41c29bd/chi_sim.traineddata) | [Apache-2.0](https://raw.githubusercontent.com/tesseract-ocr/tessdata_fast/87416418657359cb625c412a48b6e1d6d41c29bd/LICENSE) | Bytes fetched into memory to hash, not installed or bundled: 2,469,156 bytes; SHA-256 `a5fcb6f0db1e1d6d8522f39db4e848f05984669172e584e8d76b6b3141e1f730`. No model execution. |
| PDFium | [commit 547d3f4ba2a309f059e4d0ccc12490690c1c23c7](https://pdfium.googlesource.com/pdfium/+/547d3f4ba2a309f059e4d0ccc12490690c1c23c7) | [BSD-style project license plus third-party notices](https://pdfium.googlesource.com/pdfium/+/547d3f4ba2a309f059e4d0ccc12490690c1c23c7/LICENSE) | Source target only. No native artifact, artifact checksum, ABI validation or resource acceptance. Official build defaults enable JS/XFA: explicitly require `pdf_enable_v8=false`, `pdf_enable_xfa=false`, and do not call document actions. [Build instructions](https://pdfium.googlesource.com/pdfium/+/547d3f4ba2a309f059e4d0ccc12490690c1c23c7/README.md). |
| quick-xml | [v0.42.0 official release](https://github.com/tafia/quick-xml/releases/tag/v0.42.0) | [MIT](https://raw.githubusercontent.com/tafia/quick-xml/v0.42.0/LICENSE-MIT.md) | N5 evaluation target, not installed in N3. Requires bounded XML tokens/characters and explicit DTD/entity/external-link denial. |
| zip | [v9.0.0 official release](https://github.com/zip-rs/zip2/releases/tag/v9.0.0) | MIT, fetched crate manifest and LICENSE | **Installed/pinned `=9.0.0`**, default features disabled, only `deflate-flate2-zlib-rs`. Synthetic stored/deflate ZIP reader tests pass; no OS sandbox or complete OOXML acceptance implied. |
| calamine | [v0.36.1](https://github.com/tafia/calamine/releases/tag/v0.36.1), [official manifest](https://raw.githubusercontent.com/tafia/calamine/v0.36.1/Cargo.toml) | MIT | Evaluated as a candidate by source/manifest only, not installed or benchmarked. Prefer transparent bounded OOXML reading in N5; do not assume calamine exposes all row/column/formula/external-link controls. No claim it failed a runtime test. |

The source-only candidates above are not distribution approval. Native binaries must come from a recorded build/source chain, with architecture, build options, dependency licenses and SHA-256 recorded before use. No NapCat binary is acquired or redistributed.

New Cargo lock entries (registry SHA-256 values are preserved in `Cargo.lock`; detailed local provenance is `N3-dependency-provenance.json`):

| Package | Version | License from fetched crate manifest |
|---|---|---|
| zip | 9.0.0 | MIT |
| flate2 | 1.1.10 | MIT OR Apache-2.0 |
| zlib-rs | 0.6.8 | Zlib |
| crc32fast | 1.5.2 | MIT OR Apache-2.0 |
| equivalent | 1.0.2 | Apache-2.0 OR MIT |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 |
| indexmap | 2.14.2 | Apache-2.0 OR MIT |
| typed-path | 0.12.3 | MIT OR Apache-2.0 |

## Portable boundary available to N4/N5

`shixu_native::attachments::container::read_bounded_archive(bytes, limits, member_callback) -> Result<DetectedType, PartReason>` reads a borrowed, already bounded byte slice. The callback receives a raw validated relative member name and one bounded member's bytes. Buffer tentative extraction; discard all output on any final error. Do not perform database writes or external actions inside the callback.

Before ZIP parsing, compressed bytes are capped at 20 MiB and the original EOCD entry count at 5000. ZIP64, multidisk, self-extracting layouts, encrypted entries, non-stored/non-deflate methods, directories, links, traversal, duplicate names and `.bin` content are rejected. Declared compressed ranges are bounded before reading and raw deflate consumption must exactly match that range; padded/trailing compressed bytes cannot inflate the compression-ratio denominator. Actual decompressed output is counted on each 8192-byte read, capped cumulatively at 100 MiB and per member at 100:1. CRC/read failures and declared/actual output-size mismatches reject the archive. `zip` indexes duplicate names silently; the wrapper also compares the original central-directory count with the index count to close that ambiguity. Resource arithmetic is checked; overrides can only tighten v0.1 caps.

DOCX/XLSX return values mean **structural marker classification only** (`[Content_Types].xml` with `word/document.xml` or `xl/workbook.xml`). They do not validate content-type XML, relationships, external links, formulas, hidden rows/columns, or text semantics. N5 must add those checks, 200,000 extracted characters/file, 10 sheets, 2000 rows/sheet, 50 columns/sheet and 20,000 nonempty cells/file. Callbacks are tentative until the final return. No XML reader exists yet. N4 must implement real image decoding/pixel validation and PDF page counting/rendering under isolation. The 20-page and other numeric policies are tested, not real engine behavior.

`ParserLimits::v01` preserves the established DTO values. `check`, `check_image`, `check_message`, `ArchiveBudget` and `summarize_parts` are portable helpers. Mixed failures remain `PartialParse`; no aggregate implies `NonEvent`. Existing wire state `LimitExceeded` is retained instead of adding the brief's illustrative `OverLimit` variant.

## Download and storage scope

`DownloadService<DownloadPort, Clock>::fetch_attachment(reference, limits)` makes context explicit and returns a protected cache path. `fetch_detailed` retains stable `PartReason` errors; `download_failure` maps them to durable per-part statuses. There is no global resolver, credential store or fake production encryption.

Only a trusted adapter may resolve an authenticated source/message/part reference tuple. Endpoint host/path exist transiently and have no Debug/Serialize implementation. The service accepts exact lowercase DNS-host allowlist matches, HTTPS-only transport contracts, at most three separately approved redirects, expiration and original-vs-thumbnail checks, bounded streaming reads and optional SHA-256 integrity. Missing originals, expired references, authentication failures, unsupported formats and thumbnails remain distinguishable. A transport must enforce TLS, peer-address policy including DNS rebinding, port 443, no automatic redirect following/ambient cookies/proxy, and connect/read deadlines. **There is no production HTTP/QQ downloader yet; G2 remains UNVERIFIED.** Synthetic transport tests are not actual SSRF/network-isolation evidence.

The current download preflight accepts PDF magic and PNG signature/IHDR dimensional bounds. It is not PDF parsing or image decoding; JPEG/ZIP/OOXML acquisition is deliberately unsupported until an isolated validation path exists. The separate ZIP reader is callable for controlled synthetic fixtures only and is never an in-process product fallback.

Cache files contain only `DataProtector` output, use opaque message/part UUID names, and are created without overwrite. Plain input and work buffers are zeroized on drop; no temporary plaintext files are used. `DataProtector` is required explicitly; the only product implementation remains DPAPI, which fails closed off Windows. Synthetic test protection exists only in the integration-test binary. The Unix test cache uses a private 0700 directory, 0600 files, symlink checks and exclusive OS file locking; Windows cache construction returns `Unsupported` pending ACL integration. Cache use is counted from actual protected file lengths under the exclusive lock, including after reopen; 1 GiB causes visible refusal, never eviction. Per-message protected byte totals/part counts conservatively enforce 50 MiB/5 parts. Protection must not shrink data; otherwise the store refuses it. Encryption overhead and retained prior revisions can therefore reject a nominally in-range download earlier. No automatic deletion of linked originals exists; lifecycle/manifest integration and 30-day original cleanup remain downstream work.

## Durable jobs and revision rule

SQLite migration **3** adds attachment task state, retry count, due time and opaque lease; no reference URL or content is stored in that table. `TaskQueue` reserves at most 100 incomplete jobs and one active claim across database instances. Unknown source sizes are conservatively reserved at the type maximum. Limits do not block message-body persistence. Reopen retains jobs/retry deadlines; transient acquisition failures alone get three retries after 1, 5 and 30 minutes. Authentication/expiry/thumbnail/unsupported outcomes are terminal. Startup recovery invalidates old leases only after a native supervisor proves all former children stopped; a timer alone is insufficient. Stale running jobs retain the slot until that recovery so an edit cannot silently permit a second child.

`MessageStore::record_parts(key, expected_revision, parts)` now requires the dispatch revision. The revision check, evidence merge and envelope write share a single immediate SQLite transaction. `TaskQueue::finish` checks its lease and performs that same write plus task completion atomically. Edits reusing part IDs, revocations, superseded leases and duplicate completions cannot write stale evidence or complete a current task. There is no legacy unguarded write overload. D1 must independently compare its batch revision in its calendar transaction.

## Acceptance matrix and native work still required

| Gate | Status | Required evidence to change status |
|---|---|---|
| Portable limits, revision protection, jobs, injected transport/cache and bounded ZIP contracts | PASS on Linux synthetic tests | Reproduce recorded tests; this status is scoped to those contracts. |
| Windows parser isolation | **BLOCKED — not implemented or exercised** | Restricted token/AppContainer or equivalent independently verified network denial; private input ACL/handle allowlist excluding vault/credentials; no inherited ambient handles; Job Object kill-on-close, active-process limit and 512 MiB memory cap; 30-second image/120-second file watchdog termination. |
| Actual network/vault denial | **BLOCKED** | A real worker attempts outbound TCP/UDP/DNS/loopback and reads a controlled vault directory; record OS-denied failures and zero server-observed requests. Repeat after restart, worker crash, timeout and malicious spawn. Mocks cannot pass this gate. |
| Protected Windows cache/input handoff | **BLOCKED** | Reviewed ACL/reparse-safe Windows filesystem operations, DPAPI production cache proof, read-only inherited handle binding, plaintext handle lifetime/cleanup and crash residue inspection. |
| OCR/PDF engine resource/quality | **BLOCKED** | Fixed native artifacts/model hashes; real boundary and hostile fixtures; measured peak child memory/time under actual restrictions; PDF JS/XFA actions disabled; quality sets separately reported. No silent increase above 512 MiB. |
| OOXML semantic extraction | **BLOCKED / N5 portable work can proceed** | Bounded XML implementation plus formulas/external links/row-column limits and per-format quality evidence; still no product dispatch before Windows gate. |
| Real QQ file acquisition | **UNVERIFIED / G2** | Authorized actual source reference/download tests, real authentication/expiry/redirect behavior and approved egress policy. |

`cargo test -p shixu-native --test parser_isolation -- --ignored` currently **fails (exit 101)** with an explicit BLOCKED message. It is a visible unresolved gate, not an ignored test counted as a pass. No real Windows probe has been run here.

One Linux synthetic ZIP case (valid small container, traversal, 1 MiB repeated-data bomb) was measured by Python `resource.getrusage` around the already-built test executable: exit 0, 0.0613 seconds, peak RSS 9344 KiB. This measures that one test harness only, not worst-case resources, an engine, or a Windows Job Object. Ordinary `/usr/bin/time` was unavailable; the measurement used the standard Python resource API. Local raw evidence: `.superpowers/sdd/shixu-v0.1/N3-resource-synthetic.log`.
