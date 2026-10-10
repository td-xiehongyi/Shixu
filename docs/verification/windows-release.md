# Windows 首版发布门槛（D7）

2026-10-10 剪贴板需求修订：复制账号/密码后不因计时、锁库或退出自动清除；内容保留至用户覆盖或手动清除，可能留有密码。15 秒掩码、5 分钟闲置锁库、会话撤销和瞬时秘密内存清零保持要求；原生复制、历史/云同步排除及权限隔离仍须真实验收。
2026-10-09：**发布 BLOCKED**。云端可运行严格证据校验器及 portable 测试；Windows 包、真实密码库、QQ 接收、剪贴板、进程隔离、全格式质量尚未验收。此文档及脚本不发布软件。默认测试中的 ignored 原生门槛不是通过，显式选择它会失败并说明 BLOCKED。

## 当前实现与证据边界

`src-tauri/src/release.rs` 定义版本 1 的报告结构和必需门槛。实际 `shixu-desktop` 可执行文件提供两个显式命令，均在启动桌面运行时之前处理：

```text
shixu-desktop --release-manifest
shixu-desktop --verify-release EVIDENCE_OR_DASH OUTPUT
```

第二项以 `-` 表示未提供证据，写出所有必需门槛 BLOCKED 的报告。`OUTPUT` 必须是尚不存在的新文件；实际 CLI 以原子 create-new 创建并通过已打开的句柄写入，拒绝所有已存在的输出（包括原报告、证据/二进制、相对路径别名、符号链接及硬链接），返回 1 并保留原文件。每次使用新的报告文件名，不覆盖或删除旧证据/报告；写入失败时新文件可能不完整，不能当作验收报告。退出码：0=所有必需门槛 PASS；1=FAIL（含非法证据）；2=BLOCKED。读取失败、空缺、未知字段/枚举、重复 JSON 对象键或门槛/覆盖项、超过 1 MiB、非法类型/数值、错误版本/构建/来源/可执行文件摘要均不能通过。输入错误详情不会回显证据载荷。日志引用不接受任意路径；校验器不读取密码文件。

编译时 build script 使用 Git 列出的已跟踪及未忽略新增文件的路径与实际字节生成 source_sha256；source_commit 记录当时 HEAD，**不是 dirty 构建的唯一身份**。build_id 绑定 source_sha256、目标平台、构建 profile、应用版本；versions 包含应用、Tauri 和两个依赖锁文件摘要。可执行文件运行时计算自身 artifact_sha256；证据必须匹配这些实际编译/文件值，不能从证据 JSON 提供预期能力。新增/修改源文件应重新构建并重新收集证据，不能复制旧 build_id。构建需要 Git checkout；未提供无需 Git 的发布源码打包模式。

capabilities 是代码维护的当前能力清单，必须在真实实现、审阅和验证之后才更新。它不是从操作员 `passed`、程序启动或 ignored 计数推导的。当前 native 能力为 unimplemented / written_untested / written_untested_incomplete / quality_open，即使所有输入声称 PASS，仍有 BUILD_CAPABILITY_BLOCKED。Linux 编译目标额外产生 WINDOWS_BLOCKED。optional 云端模型未选定、默认关闭，不是必需 baseline provider 门槛。

SHA-256 在这里用于绑定实际本地构建和文件身份，**不是签名、远程证明或对操作员陈述的密码学证明**。校验器检查结构、范围、覆盖、声明的组件/日志元数据及指标；不自行运行 QQ、引擎、网络探针，也不验证引用日志的真实性。审阅者还须独立检查原始合成测试、组件来源、日志文件摘要/直接退出码及实际 Windows 行为。任何输入报告都不能替代缺失的产品实现。

当前 `SystemClipboard` 有新的同步 write-only Win32 源码，clipboard能力为 `written_untested`，绝不是 `native_implemented`。非Windows仍Unsupported，vault dispatcher仍Unsupported；安装组件不能启用复制UI。暂停的所有权草稿仍归档，未恢复。实际共用编码/发布链有portable行为测试，Windows专用合成测试显式ignored；GNU Rust cfg/type-check不链接、不运行Win32/SQLite，不能关闭门槛。

Windows调用使用同线程临时nonnull owner HWND、立即渲染及GMEM_MOVEABLE；注册格式与分配全部在Open/Empty前完成。先发布 `ExcludeClipboardContentFromMonitorProcessing`、`CanIncludeInClipboardHistory` DWORD0、`CanUploadToCloudClipboard` DWORD0，再发布CF_UNICODETEXT；任一步失败返回固定错误，无不受保护的文本fallback。成功的HGLOBAL由Windows所有，程序绝不清零/释放它；只清零未发布缓冲和暂存秘密。Close/Destroy失败也报告错误，清理只释放本地资源，不再次EmptyClipboard。Microsoft契约见 [SetClipboardData](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setclipboarddata) 与 [历史/云格式](https://learn.microsoft.com/en-us/windows/win32/dataxchg/clipboard-formats)。

输入必须为UTF-8、无内嵌NUL且≤65536字节；保留Unicode、空白与原始换行，不裁剪/截断。任意SecretBytes并非都能表示为CF_UNICODETEXT，这项不兼容保持原生验收BLOCKED。准备或Open失败不会清旧内容；Empty成功之后再失败可丢失旧内容或只留下部分排除格式，Close/owner清理失败甚至可在报错时已经留下文本。fake端口“失败保留旧内容”不是OS事务保证；不回滚/读回/清除已发布内容。无generation/marker/sequence观察、定时、锁库、退出或Drop剪贴板清除。新的覆盖项是 `copy_persists_until_user_overwrite_or_manual_clear`；旧 `copy_30_seconds_generation` 证据拒绝。

用户明确选择可覆盖的合成测试剪贴板后，在交互式Windows账号运行：

```powershell
cargo test -p shixu-native --test clipboard_policy --locked windows_synthetic_unicode_and_exclusion_formats_are_published -- --ignored --exact --test-threads=1
```

它真实写入并读取合成文本/排除DWORD，验证owner销毁后立即渲染数据仍在；Linux不编译这项，不能以零测试通过代替执行。它不验证vault权限、历史UI/云同步、15秒掩码或5分钟闲置锁库。完整验收还须实际粘贴、跨计时/锁库/退出观察持久性、用其他程序覆盖/手动清除，启用Windows历史/云同步的受控合成测试并确认排除，检查固定错误和写入失败边界；不得把一次单元测试当完整clipboard PASS。优先顺序及用户准备见 [最小首次运行链](minimal-first-run.md)。

## Windows 手工准备与运行

1. 在用户自己的 Windows 10/11 测试机保留合成数据专用目录，记录 Windows 版本、架构和测试 Windows 身份。准备一个真正不同的 Windows 身份做 DPAPI 拒绝测试。不要把真实密码、访问令牌、聊天导出发到对话中。
2. 用户独立安装 Microsoft C++ Build Tools 的“Desktop development with C++”工作负载、Rust MSVC host、Node/pnpm；checkout 使用仓库的 Rust **1.99.0** 和 pnpm **11.19.0** 固定版本。构建还需要 Git。官方步骤见 [Tauri Windows prerequisites](https://v2.tauri.app/start/prerequisites/#windows)。云端观察的 Node 为 24.19.0；Windows 构建尚未跑过，不代表该组合已在 Windows 通过。
3. 检查实际 WebView2 Runtime 是否存在，缺失时由用户通过微软官方安装程序准备。微软说明少数 Windows 10 设备仍可能缺少 Runtime；记录实际 Runtime 版本并做运行验证。[Microsoft WebView2 distribution](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution)。脚本不会下载或安装 Runtime。
4. 在仓库根自行执行下列本地构建/检查。`bundle.active=false` 保持不变；`pnpm run tauri build` 只是本地 Windows 二进制构建尝试，**当前不会生成 Windows 安装包**。打包目标、安装/卸载及 WebView2 缺失处理仍待实现和验证，未声称有 MSI/NSIS 包。

```powershell
pnpm install --frozen-lockfile
cargo test --workspace --locked
pnpm run test
pnpm run typecheck
pnpm run build
pnpm run tauri build
cargo test -p shixu-desktop --test windows_release --locked -- --ignored
```

最后一项当前刻意失败：它是未实现真实端到端门槛的哨兵，不会用 mock 跑一遍后假装通过。Windows 编译/链接也可能暴露尚未测过的 cfg 集成差异。不要把前端构建成功当作 Windows 二进制/安装包成功。

5. 二进制确实构建成功后，使用 PowerShell 7 `pwsh` 手工调用 wrapper（从仓库根，按实际产物位置调整）：

```powershell
.\target\release\shixu-desktop.exe --release-manifest
pwsh -NoProfile -File scripts/verify-windows-release.ps1 `
  -Executable .\target\release\shixu-desktop.exe `
  -Output .\release-report.json
# 有真实、同构建的脱敏证据时才增加 -Evidence .\release-evidence.json
```

wrapper 只调用指定二进制的校验命令，不构建、登录、发送、上传、发布或安装任何东西。输出必须是尚不存在的新文件；wrapper 使用实际 CLI 的原子拒绝覆盖契约，每次调用请选择新的报告名。wrapper 已在云端编写，**未在 Windows/PowerShell 执行**。Linux 可直接使用 portable 二进制的同名命令检查 BLOCKED 报告，不能据此关闭 Windows 门槛。

## 所需组件与真实证据

G1：用户从 [KeePassXC 官方 Windows 下载页](https://keepassxc.org/download/#windows) 独立准备可信组件；2026-10-09 查阅页面列出 2.7.12、Windows 10/11 x64、SHA-256/PGP 链接及 MSVC Redistributable 要求。固定实际版本/来源/许可/摘要，不自动随包分发。安装或启动 KeePassXC 只完成准备。产品 VaultEngine、私有会话协议、UUID 定位和 KDBX 读写尚未实现。接通后必须以虚构三字段完成 Shixu 写入→KeePassXC GUI 核对/修改→Shixu 重开，逐字验证 UUID、Unicode、斜杠、引号、空格、换行、同名条目；主密码仅经私有 IPC，不能出现在 argv/env/日志/临时明文。还需错误密码、截断/篡改、KDF 限额、原子保存、冲突/占用/磁盘满/崩溃、改密新旧密码、备份恢复与锁定迟到响应等证据。若独立 CLI 不能满足私有会话和可逆协议，应评估并审批引擎路线修订；不要缩减字符范围或实验性写库来冒充成功。

G2：用户独立选择和配置合法许可的第三方 QQ 适配器、专用测试账号、授权专用测试群、群白名单及时区，并在自己的机器本地登录。当前 QQ wire/真实下载未实现；不存在可执行的 vault/QQ 验证专用脚本。完成实现后，测试至少 100 条已知合成群消息、8 小时后台，在线接收到持久化 P95 ≤5 秒、规则入历 P95 ≤10 秒；保留真实普通群送达、原件下载（图片/PDF/DOCX/XLSX）、断网/登录失效/重连/遗漏区间证据。心跳或已启动不等于普通群已验收，重连不自动证明缺失已补齐。此脚本不会替用户登录或发送测试消息。

其他门槛：实际 Windows WebView IPC 权限隔离、托盘/退出、会话锁定/休眠/恢复、自启注册、第二进程聚焦；同/不同用户 DPAPI、ACL/reparse、保护缓存和只读输入交接；15 秒显示、复制后保留至用户覆盖/手动清除且计时/锁库/退出不自动清空、剪贴板历史/云同步排除和vault-only复制权限、5 分钟闲置锁库且后台不刷新；真实 parser Job Object 512 MiB/子进程限制/kill-on-close、无网络/无密码库、30 秒单图/120 秒单文件超时。whole_flow 要有自动事项来源证据、改期取消、人工覆盖、撤销、重启防重放及确认恢复观察，不能只有“打开成功”。这些 native 门槛目前均 OPEN。

## 输入版本 1

先从**待验收的那个二进制**读取 `--release-manifest`。顶层字段恰为：`schema_version=1`、`build_id`、`source_sha256`、`artifact_sha256`、`target`、`versions`（完全匹配编译清单）、`gates`。未提供 gate 留空会 BLOCKED；禁止输入自行声明的 capabilities 或 passed:true。

每项 gate 恰含：`id`、`status`（PASS/FAIL/BLOCKED/UNKNOWN/STARTED）、`kind`（native/portable/synthetic）、`started_at`/`finished_at`（非负 Unix 毫秒）、`coverage`、`metrics`、`components`、`logs`。结束不得早于开始、晚于校验时刻或距校验超过24小时。超长/无效时间不能溢出校验器。QQ duration_ms 必须在实际声明的区间之内且至少8小时。coverage 必须完整且恰来自代码中该 gate 的固定清单；重复/未知覆盖项 FAIL。代码 `policies()` 是准确完整的必需覆盖清单。

组件项包含 `name/version/sha256/source/license`，非空文本≤256字节且无控制字符、摘要为64个小写十六进制字符，不能重复组件名；native 需要组件元数据。日志项包含 `sha256/bytes/exit_code`，同一门槛不允许重复日志摘要，bytes 在1..1GiB，PASS需要至少一个直接退出0的日志。请保留对应脱敏日志供独立核查；这些元数据不是自动文件真实性检查。

metrics 只接受可选非负整数 `messages,duration_ms,persist_p95_ms,rules_p95_ms,cases,gold,tp,fp,fn,ambiguous_guesses,sourced_events,undone_events`；具体门槛的必需指标缺少就不能通过。质量数量≤1,000,000，TP+FN=gold，精度≥95%、召回≥90%、含糊猜测0。quality_text 必须恰绑定已冻结的157/132/123/3/9数据集；附件需完整每类型至少20例（DOCX/XLSX冻结为25例各自报告），不可删掉正例/unsupported 来缩小分母。即使输入指标声称过线，当前编译 attachment quality_open 仍 BLOCKED。来源/撤销计数必须非零才能满足 whole_flow 观察字段。

## 冻结质量与未通过项

文字：157例、132 gold，TP123/FP3/FN9，P97.619%/R93.182%，含糊猜测0，只通过其 portable corpus。

N4 图片20例：完整区域29/31、定位39/47；PDF20例：eligible区域72/73、定位77/81，另有超限PDF的44个gold区域零输出单列，未掩盖。中文图片/扫描页每例仅1/2区域、6/10定位；这不是通过中文 OCR 或最终事件95/90门槛。具体所有例及范围见 [N4 quality](N4-quality.json)。

N5：冻结 DOCX25/XLSX25 全部保留（包括安全拒绝/复杂格式/unsupported gold），未用可解析子集替代。如下 TP/FP/FN 计数对应所有冻结 gold：

| 格式/指标 | TP/FP/FN | 精度/召回 |
|---|---|---|
| DOCX字符 |194/0/179|100%/52.01%|
| DOCX标题 |8/0/9|100%/47.06%|
| DOCX入历 |6/2/11|75%/35.29%|
| XLSX字符 |196/0/114|100%/63.23%|
| XLSX标题 |7/0/6|100%/53.85%|
| XLSX入历 |4/3/9|57.14%/30.77%|

含糊日期猜测0；全格式质量仍 OPEN。这些低召回/精度问题、真实引擎和 Windows 集成需要实际实现或明确设计修订，用户安装组件本身无法修复。云端通过、portable 独立审阅通过、构建身份报告生成，均不意味着首版可使用真实密码或已完成发布验收。
