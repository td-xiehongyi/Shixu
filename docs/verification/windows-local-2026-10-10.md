# Windows 本机接手与阶段验收（2026-10-10）

状态：**部分原生检查通过；最小密码链路失败；发布仍 BLOCKED**。
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

上述代码已独立审查。未修改生产密码准入、隔离权限、剪贴板策略或发布门槛，也未移除失败测试。

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

1. **密码最小链路失败。** 原生边界测试在首次创建库之前返回 `Unsupported`。临时、已移除的诊断定位到 `CreateProcessW` 返回 Win32 203；资源校验已过，进程尚未创建，尚未进行 Job 分配或 helper 请求。只补 `SystemRoot` 的一次测试实验仍失败；单独 AppContainer profile 创建/路径/删除探针成功，不能由此推定进程启动已满足条件。已暂停反复尝试，不放宽隔离或硬编码准入。真实原因仍待定位。
2. **完整 Windows 工作区回归失败（exit 101）。** `dpapi_restore` 与 `parser_isolation` 的受保护附件缓存仍在非 Unix 返回 `Unsupported`；`image_pdf` 缺少固定 N4 引擎/模型。保留所有失败，未跳过或删测试。Office 格式质量仍按原报告保留不足，不宣称全格式可用。
3. **未验证。** 密码库创建/CRUD/重开/换密/错密拒绝、实际锁屏休眠与回复投递、原生剪贴板行为、交互 WebView、第二身份、真实 QQ、安装和卸载。

公开 Windows 密码构造器继续 `Unsupported`；全部 12 个发布门槛继续 BLOCKED；历史附件原始字节丢失事件 Q1 继续 OPEN。

下一步先把 Win32 203 提炼成无秘密的最小 AppContainer 启动复现，区分环境块、可执行路径准入与启动属性。证据明确并修复后，重新运行 `scripts/test-windows-vault-minimum.ps1`，从虚构密码库创建开始；通过前不进入打包安装验收。真实 QQ 组件及客户端精确版本、获准测试群和本地连接配置仍待准备，登录和令牌由用户在本地完成。
