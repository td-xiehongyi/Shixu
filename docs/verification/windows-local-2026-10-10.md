# Windows 本机接手与阶段验收（2026-10-10）

状态：**固定 Windows 最小工作流与基础桌面操作通过；锁屏/电源待验；生产与发布仍 BLOCKED**。
检查分支 `codex/shixu-v0.1`，远端已知基线 `ba09c07716394a6d1517a17a77ec4880b5d0d59c`。
开始前工作区干净；远端复查未发现比该基线更新的开发提交。
本次只用独立测试目录、虚构密码和合成消息；未访问真实密码库、登录 QQ、输入真实令牌或运行安装包。

## 本机工具链

Windows 11 x64，10.0.26100，标准用户。实际工具：

| 项目 | 检查结果 |
| --- | --- |
| Git | 2.54.0.windows.1 |
| Rust | 1.99.0，MSVC host/target；真实编译、链接和执行成功 |
| C++ | VS 2026 Build Tools 18.8.2 / MSVC 14.51.36231；另有 VS 2022 Community 17.14.14 / MSVC 14.44.35207 |
| Windows SDK | 10.0.26100.0 |
| 前端 | Node 24.19.0 / pnpm 11.19.0，匹配仓库固定版本 |
| Python | 3.14.7 |
| WebView2 | 注册表检测 Runtime 154.0.4258.62；尚未进行交互运行验收 |

首次在仓库内查询 rustup 时，仓库固定配置触发了 Rust 1.99.0 自动准备；这是只读检查阶段的一次意外写入，已向用户说明，未回滚或改动已有 stable 工具链。
随后用户明确授权按锁文件准备公开依赖；已完成 frozen pnpm、locked Cargo，以及固定 Node 26.11.1 / KdbxWeb 2.1.1 / hash-wasm 4.12.0 密码资源。锁文件、版本与审核过的资源清单均未变更。

## 本轮修复

- Windows `core.autocrlf=true` 改写四个固定 helper 输入的换行，导致资源哈希失败。四路径固定 LF，并增加真实 Git checkout 字节回归；原资源哈希未修改。对应提交 `4eaeb8f`。
- 日历备份在 Windows 用只读句柄调用 `sync_all`，实际刷盘失败。Windows 重新打开现有快照时取得读写权限，不创建或截断文件，并继续传播错误。
- 连接测试清理目录前释放仍持有 SQLite 的接收端引用，修复 Windows 文件占用失败。
- DPAPI/ACL 原生检查的 Windows PowerShell 子进程不再继承 PowerShell 7 模块路径；ACL 脚本遇到错误立即失败，保留原权限断言。

上述代码已独立审查。未修改生产密码准入、剪贴板策略或发布门槛，也未移除失败测试。

## 第二轮：密码进程启动与原生探针修复

接续提交 `9e46e8d` 时工作区干净，远端复查仍为同一提交，`main` 为 `903a9c0`。本轮先用无秘密 C++ 最小探针比较环境块，随后用固定虚构 KDBX 数据定位；没有访问真实密码库、修改系统安全设置、增加 AppContainer 能力或升级固定运行资源。

- **启动修复：** 空环境导致本机 `CreateProcessW` 返回 203；仅提供系统 API 获取的 LOCALAPPDATA 后可创建实际 AppContainer 进程。固定 Node 随机数还需要系统 API 获取的 SystemRoot。最终环境仅这两个条目，严格本地固定磁盘校验，不继承 PATH、NODE_OPTIONS、TEMP 或用户凭据。
- **模块加载修复：** Node 默认模块 realpath 探查未授权的磁盘根目录，实际 EPERM。增加 preserve-symlinks 与 preserve-symlinks-main 后固定 helper 可以初始化；原 198 文件哈希、无 reparse 准入及句柄固定保持，不授予根目录 ACL。
- **句柄探针修复：** 原探针对未继承的父端编号直接进行子端文件信息查询，触发严格句柄异常 `0xc0000008`。父端现于 child resume 前查询实际子端句柄表；只接受 ERROR_INVALID_HANDLE，保留有效复制正对照与文件身份比较。
- **父退出探针修复：** 可信测试监督进程清空环境后使用不可写的系统临时目录。仅为此测试宿主指定已准入合成目录父目录的 TEMP/TMP，严格路径校验不变；被隔离 helper 仍只接收两个原生元数据条目。

新增精确回归覆盖系统元数据环境、挂起启动、固定 helper 初始化/EOF、无凭据 LOCKED 回复，以及独立 OS 权限/网络/Job/内存和真实父退出检查。各直接运行均通过；相关红绿日志保留。独立审查未发现阻断问题；格式和实际 Windows 全工作区/all-targets 严格 Clippy 通过。

本轮原始诊断与开发回归日志：`docs/verification/local/appcontainer-start-8c439b2440964df2881626162f039be1/`。临时 Node eval 诊断已从源码移除，固定 helper 字节与清单未修改。

完整 `synthetic_native_boundary` 已实际运行并通过（直接 exit 0，167.74 秒）：虚构库创建、增删改查、加密保存、关闭重开、改主密码、旧/错误密码拒绝与换行校验；实际 token/Job、私有库与日历文件访问拒绝、资源只读、inbox 写入、句柄身份、带对照的 TCP/UDP 拒绝、进程/内存限制；资源校验、冲突、加密检查点故障恢复、取消、真实父退出与 Job 关闭、阻塞 IO 及 30 秒超时。固定记录中的四个阻断是第二身份、物理断电、真实磁盘满和创建 reparse 夹具所需权限；没有伪记为通过。

原封不动的 `scripts/test-windows-vault-minimum.ps1` 随后在干净提交 `c638668da3a9d04e07c1cf1357382a4a1e175729` 上完整通过（直接 exit 0）。包含固定离线依赖/资源、前端类型与构建、路径构造器、完整原生边界、custom-protocol MSVC 桌面构建及三个精确生命周期控制器回归；最终所带 198 个资源全部匹配原清单。结果只绑定这个源码提交，之后的报告更新不冒充重新构建。

- 最小工作流证据：`.superpowers/sdd/shixu-v0.1/task-windows-vault-lifecycle-native-a1cc9eab1aa64aaa977886c942b72600/`。
- 对应原生边界证据：`.superpowers/sdd/shixu-v0.1/task-windows-vault-boundary-native-a9e290d6c9d64bd6b50dce37fa25f673/`。
- 桌面 EXE SHA256：`25af6161fb85c595bde5799e2bc9f338b37ac09de50248dee10289da361b920b`。这是工程 debug 产物，不是已安装发行包。

已用固定观察脚本打开实际桌面窗口，日志确认 `mode=fresh-synthetic-unavailable production_store=untouched` 及 `registration=ready production=Unsupported`；进程有“拾序”主窗口。证据目录 `.superpowers/sdd/shixu-v0.1/task-windows-vault-lifecycle-observe-b975a9aa923d4915ba04a3ab33b74b2c/`。用户确认“可以正常点击密码库，可以从托盘退出”；日志出现 WindowClosed/Revoked，进程随后 exit 0，产物哈希未变。这只验收基础交互，不证明密码可解锁、全部 UI 或系统锁屏/休眠通知。

退出时 stderr 另有 `Chrome_WidgetWin_0` 窗口类注销错误 1412，原因未定位；用户没有报告对应操作失败，仍保留为待定位诊断。日志未见实际 session lock/unlock 或 suspend/resume，用户也未确认这些步骤；这部分仍未验证，不记通过。

## 已取得的证据

- 前端测试、类型检查和生产前端构建通过。
- Windows 日历备份恢复、QQ 连接控制、OneBot 合成 loopback socket 到持久来源及日历的测试通过；这不是登录真实 QQ 的 G2 证据。
- 实际当前用户 DPAPI 加解密、篡改拒绝、目录与 SQLite 文件权限检查通过；其他身份拒绝尚未验证。
- Windows 可信资源路径构造器检查通过。
- `custom-protocol` 桌面程序真实 MSVC 编译链接通过；产物所带 198 个运行资源全部匹配原清单，无 reparse point。没有据此认定 WebView、安装或生产密码可用。
- Git 换行回归、Rust 格式检查及 Windows 全工作区/all-targets 严格 Clippy 检查通过。

原始本机输出位于忽略目录，不随代码推送：

- `.superpowers/sdd/shixu-v0.1/task-windows-vault-lifecycle-native-ded6cbbae70a4ef09182355068897d1e/`
- `.superpowers/sdd/shixu-v0.1/task-windows-vault-boundary-native-2c49ea71085e4ef9bae31b38a5ae3c67/`
- `docs/verification/local/windows-stage-c04bf4145c5a4b739788ff3de9344266/`：完整工作区输出、各直接退出码、当前用户 DPAPI/ACL、桌面构建及源码/产物身份。

## 真实失败与阻断

1. **第一轮失败已定位并修复。** 首次原生测试在创建库前的 Win32 203、随后随机数及测试探针问题，均按上节取得红绿证据。第二轮完整原生合成链与干净提交的固定最小工作流已通过。公开构造器的非 Linux 准入拒绝仍明确保留，不能用此次合成结果直接删除生产门槛。
2. **完整 Windows 工作区回归失败（exit 101）。** `dpapi_restore` 与 `parser_isolation` 的受保护附件缓存仍在非 Unix 返回 `Unsupported`；`image_pdf` 缺少固定 N4 引擎/模型。保留所有失败，未跳过或删测试。Office 格式质量仍按原报告保留不足，不宣称全格式可用。
3. **未验证。** 真实桌面密码接线与显示/复制、实际锁屏休眠与回复投递、原生剪贴板行为、其余 WebView 交互与退出诊断、第二身份、真实 QQ、安装和卸载。原生合成库和基础打开/退出通过不代替这些结果。

公开 Windows 密码构造器继续 `Unsupported`；全部 12 个发布门槛继续 BLOCKED；历史附件原始字节丢失事件 Q1 继续 OPEN。

下一步重开固定独立观察模式，完成用户手动锁屏/解锁与睡眠/唤醒，核对真实 WTS/电源事件，并定位 WebView 退出诊断；再补生产存储准入与真实引擎桌面接线、回复和剪贴板验收，不以删除 Unsupported 分支代替实现。当前未生成或运行安装包。真实 QQ 组件及客户端精确版本、获准测试群和本地连接配置仍待准备，登录和令牌由用户在本地完成。
