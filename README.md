# Shixu / 拾序 v0.1

中文 Windows 日历与密码工作台的 portable 开发基础。KdbxWeb 独立进程、原生密码库 dispatcher 与隔离视图已接通；普通 Windows 入口使用本机正式密码库，虚构密码库保留独立验收入口。真实 QQ 接收、完整附件质量与全产品发布仍 BLOCKED；浏览器演示不代表已验收产品。

- [使用与Windows准备说明](docs/user-guide-v0.1.md)
- [Windows发布门槛、证据格式及阻断项](docs/verification/windows-release.md)
- [Windows最小MSVC构建、合成vault probe与窗口/托盘观察](docs/verification/windows-vault-lifecycle.md)
- [正式密码库本机接入与剩余验收记录](docs/verification/vault-production-admission-2026-10-11.md)

本地 Windows 已完成最小构建后，双击根目录的 **`启动拾序.cmd`** 即可启动桌面程序。入口不要求管理员权限，不自动构建、安装或连接 QQ；程序按正常模式使用本地应用数据。关闭主窗口进入托盘，完整退出请使用托盘菜单。密码库已接通本机正式存储：首次进入后点击“新建密码库”，自行设置主密码，以后输入主密码打开。库文件位于 `%LOCALAPPDATA%\app.shixu.desktop\vault\active\vault.kdbx`，不随项目目录移动。真实 QQ 与安装尚未验收。

仅检查启动文件路径（不启动应用、不访问应用数据）：

```powershell
.\启动拾序.cmd /check
```

脚本使用 `target/x86_64-pc-windows-msvc/debug/shixu-desktop.exe` 和随构建复制的运行资源，搬动项目时应保留该目录结构。路径检查不代替资源完整性或产品验收。文件缺失时按 [Windows 最小工作流](docs/verification/windows-vault-lifecycle.md) 准备并构建；当前本机已有该构建产物。

**虚构密码库验收**：双击根目录 `虚构密码库验收.cmd`，再点击“密码库”。它使用固定 `--vault-fictional` 模式与程序旁 `shixu-fictional-desktop` 专用目录，保留虚构 KDBX 供关窗、重启后重新打开；不初始化正式应用存储，不连接 QQ。仅使用虚构账号和密码，与普通入口的正式库互相独立。详见 [Windows 桌面虚构密码库闭环](docs/verification/windows-desktop-fictional-2026-10-11.md)。

密码库条目支持可选多行“描述”，加密存入 KDBX Notes，旧条目默认空；新增与编辑表单均可填写或清空。见 [描述字段验收记录](docs/verification/vault-description-2026-10-11.md)。

在仓库根（Rust 1.99.0、pnpm 11.19.0、已有依赖/测试组件）运行：

```text
cargo test --workspace --locked
pnpm run test
pnpm run typecheck
pnpm run build
pnpm run dev
```

`pnpm run dev` 可浏览界面，`?demo=1` 是明确标识的演示。Windows运行时尚未验收，`bundle.active=false`，当前没有安装包。release validator的Linux版本可输出本构建的BLOCKED证据：

```text
cargo run --locked -p shixu-desktop -- --release-manifest
cargo run --locked -p shixu-desktop -- --verify-release - /tmp/shixu-release-report.json
```

最后命令预期退出2（BLOCKED）。所有命令不发布软件；真实Windows准备和PowerShell wrapper详见门槛说明。
