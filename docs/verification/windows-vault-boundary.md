# Windows 合成 vault 边界执行

Stage B 新增私有 Windows launcher/store 和固定的原生测试，公开 `KdbxWebEngine::prepared` **仍返回 Unsupported**。Linux crypto 测试、GNU Windows cfg/type/lint 和源码中存在测试，都不证明 AppContainer/Job/ACL/MIC 或 MSVC 实际运行通过。Q1 OPEN、全部12发布门槛 BLOCKED；当前保留关窗撤销代际屏障、工程 ICO 和精确资源映射；锁屏/睡眠及唤醒逻辑与验收已由用户取消。实际本机合成链及 MSVC 构建已通过，生产桌面接线仍未验收。完整最小工作流见 [windows-vault-lifecycle.md](windows-vault-lifecycle.md)。

只用脚本自己创建的全新临时目录、固定虚构密码和仓库审核的资源；不传真实库、凭据、程序、路径、能力或环境覆盖。标准用户执行，不改防火墙/loopback 豁免，不登录 QQ，不自动安装组件，不启用生产引擎。

2026-10-10 本机排错补充：launcher 从 `SHGetKnownFolderPath` 获取当前用户 `LOCALAPPDATA` 与系统 `SystemRoot`，校验严格本地固定磁盘路径，构造仅这两个条目的环境块；不继承 PATH、NODE_OPTIONS、TEMP、令牌或其他用户变量。空环境在本机 AppContainer 创建时报 Win32 203；只有 LOCALAPPDATA 时固定 Node 的 WebCrypto 随机数失败；补入系统 API 获取的 SystemRoot 后虚构 KDBX 创建/保存/重载成功。这个结果仅针对本机固定运行资源，不是所有系统的通用兼容性承诺。[AppContainer 环境重定向](https://learn.microsoft.com/en-us/windows/win32/secauthz/implementing-an-appcontainer)、[系统目录 API](https://learn.microsoft.com/en-us/windows/win32/api/shlobj_core/nf-shlobj_core-shgetknownfolderpath)。

固定 Node 启动增加 `--preserve-symlinks` 与 `--preserve-symlinks-main`，避免模块加载时重新探查没有授权的祖先目录。资源仍须通过精确哈希、无 reparse 与持有句柄防替换校验；没有增加根目录 ACL、网络能力或原生模块权限。[Node 标志语义](https://nodejs.org/api/cli.html#--preserve-symlinks)。

OS 句柄探针由父端在子进程恢复前通过 `DuplicateHandle` 查询实际子进程表，只有 `ERROR_INVALID_HANDLE` 可表示目标编号不存在；其他错误仍失败。有效父端 sentinel 副本是正对照，发生编号碰撞时继续比较文件身份。这样保留继承拒绝验证，同时避免子端的严格句柄检查因查询不存在句柄而终止探针。[严格句柄检查](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-process_mitigation_strict_handle_check_policy)、[DuplicateHandle](https://learn.microsoft.com/en-us/windows/win32/api/handleapi/nf-handleapi-duplicatehandle)。可信父退出测试宿主单独接收已准入合成目录的 TEMP/TMP，以创建自己的夹具；这两个变量不会传给 AppContainer 密码进程。

## 缺失组件先准备，随后执行

以下命令是用户在实际 Windows x64 机器上执行的检查流程；云端没有执行它们。

1. 准备 Git、仓库固定 Rust **1.99.0 MSVC** 和 `x86_64-pc-windows-msvc` target、Visual Studio **Desktop development with C++** 工作负载及 Windows SDK。打开 **x64 Native Tools / Developer PowerShell**，不要使用 Linux SQLite cfg 环境。安装步骤由用户独立完成；脚本只检查，缺失即 BLOCKED，不循环安装。完整桌面后续还需要仓库固定 Node24.19.0/pnpm11.19.0 和 WebView2；本 native 库测试不要求 WebView GUI。
2. 将审核后的提交检出到本地，例如 `C:\src\shixu-v0.1`。需要现成 Cargo.lock 对应的离线 crate cache；`--offline` 缺包是准备缺口，不能删除锁文件或替换版本。MSVC/SDK 必须能真正链接 C 和 Windows 库；GNU cloud cfg 不是替代。
3. 用 Python3 与 npm/Node 构建工具显式准备 Windows 公共资源，或复制完全一致的审核后 prepared tree。准备命令可能下载固定官方 archive 和 npm lock 对应公共依赖，**不是应用启动动作或原生测试自动安装**。缓存使用用户 temp 下 `shixu-vault-runtime-downloads`，路径键统一 `/`。不可用 `--update-reviewed-manifest` 掩盖校验失败。

```powershell
Set-Location 'C:\src\shixu-v0.1'
rustc -vV
rustup target list --installed
Get-Command cl.exe, link.exe, rc.exe
# 输出必须包含 release: 1.99.0、MSVC host 和精确 installed MSVC target。
# 仅在用户明确准备公共依赖时执行下一行：
python scripts/prepare-vault-runtime.py --platform win-x64
# 后续只读验证不会调用 npm、下载或写资源：
python scripts/prepare-vault-runtime.py --platform win-x64 --verify-only
# 在上述组件和离线 cache 已准备后执行实际 probe：
.\scripts\test-windows-vault-boundary.ps1
```

如果 rustc/rustup、cl/link/rc、离线 crate cache 或 runtime 未准备，先解决脚本具体指出的组件缺口再运行；不要把缺失依赖当成功，不需要发送凭据、环境 dump 或真实库。Rust target 列表检查同时检查文本与直接退出码。Runner 不带自动安装或任意参数。

Runner 校验 198 个精确资源和每个 SHA256，包括 Node26.11.1、KdbxWeb2.1.1、hash-wasm4.12.0；Node archive SHA256 `97f36a8a9684ff0d3e35758b4610fef5b628a5880e96f2ccc11240e5daf9934e`，node.exe `a619e2e09eb0d50ef4c733d678b4d22689d2afef138cd67fd3fa107b535f8819`。它以 `--locked --offline --target x86_64-pc-windows-msvc` 各执行一次精确 production gate 和 ignored native parent，并拒绝0匹配或跳过测试。

每次使用独立 `task-windows-vault-boundary-native-<GUID>` 证据目录，文件 exclusive-create，保存 source HEAD/文件SHA、runtime SHA、实际测试exe SHA、Windows/compiler信息、完整安全编译/测试输出、直接退出码和固定case记录。没有秘密日志、helper stderr、原始协议或解密库内容。已有目录/文件不会覆盖。

## 新代码与验收限制

Launcher 使用 regular 零能力 AppContainer、验证 low token、suspended-start、精确stdin/stdout/stderr handle list、父端不继承、父端 unnamed Job（512MiB process/Job、1 process、kill-on-close、UI限制），在 cancellation authority mutex 内登记并 resume。30秒 request 和有界 Windows kill/wait/IO取消保留；异常终止失败不会无界 join。此异常 detach 清理及系统 API 的实际效果仍需本机证据。

父进程独占 active/lease/checkpoint 目录；helper 只接触独立低 MIC inbox 和只读资源。parent snapshot encrypted active 到 inbox；admit staged handle/ACL/MIC/owner/link/hash 后复制到 private pending，保留 flushed encrypted checkpoint，再 MoveFileEx/ReplaceFile，检查与 flush resulting active，最后刷新 inbox。任何失败关闭 engine。checkpoint 用唯一名字保留，不声称十份轮换/恢复UI已完成。

原生 parent 覆盖实际 KdbxWeb create/CRUD/reopen/revision/wrong-master/change-master、账号/渠道 CRLF允许、主密码/密码换行拒绝；token/Job query、direct Windows 文件读写删除拒绝、带父端 live controls 的 TCP/UDP、semantic sentinel handle、第二进程/实际内存请求、hash/set/pinned resource、broad ACL/remote/hardlink/lease/conflict、replacement失败和旧ciphertext解密、有界取消/Jobclose/实际父进程退出/blockedIO/30秒timeout。诊断是 test-only Rust 同一测试exe的固定 action，直接检查OS，不依赖 Node guards；没有可发布任意程序接口。可创建 symlink 时验证 leaf/ancestor reparse；标准用户缺相应 privilege 时明确 BLOCKED，不计PASS。

第二真实身份、物理断电、真实磁盘满仍单列 BLOCKED；普通权限/Job/网络失败不会增大上限、加能力/广泛ACL、改防火墙或偷偷回退。regular AppContainer 仍可访问共用系统和自身 profile 表面，不承诺 LPAC/全 registry 禁止。文件 share guard 和 cooperating lease 不构成同用户恶意 rename CAS；ReplaceFile 路径调用存在释放 guard 后的竞态，不承诺 Windows directory fsync 或物理断电安全。SACL/DACL 检查失败即拒绝；仅 inbox helper-created 继承的精确低-MIC ACL 可以无 protected 标志，private active/checkpoint 均必须 protected。

实际 Windows 的运行结果与剩余失败见 [本机阶段报告](windows-local-2026-10-10.md)；独立源码审核、实际 OS运行和剩余验收缺口解决前不能开启生产。原生复制命令已在 Stage A 经 epoch/session guard 接到 SystemClipboard，但真实 Windows/历史与云排除仍未验收。只有 master/current/new/confirmation 和存储PASSWORD禁换行，ACCOUNT/CHANNEL保留原先字符，不恢复旧的账号禁令。

历史 Stage C（WTS/电源钩子已按用户要求删除；最新状态见本机报告）：retained-main WTS/session and power subclass hooks, same-controller atomic lifecycle barrier with bounded deferred cleanup, actual clipboard EmptyClipboard/Tauri resolver final checks, native-only targeted vault_locked UI redaction, deterministic ICO and exact198-file resource map are implemented. Actual Windows execution remains unaccepted and production Unsupported. The executable minimum MSVC workflow and manual unavailable-vault observation are documented in [windows-vault-lifecycle.md](windows-vault-lifecycle.md); all12 gates stay BLOCKED and Q1 OPEN.
