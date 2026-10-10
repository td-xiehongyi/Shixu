# Shixu / 拾序 v0.1

中文Windows日历与密码工作台的portable开发基础。KdbxWeb 独立进程、原生密码库 dispatcher 与独立窗口已接通，并通过 Linux 虚构数据测试。Windows 密码库隔离、真实QQ接收、系统剪贴板、Windows端到端与完整附件质量仍BLOCKED；浏览器演示不代表已验收产品。

- [使用与Windows准备说明](docs/user-guide-v0.1.md)
- [Windows发布门槛、证据格式及阻断项](docs/verification/windows-release.md)

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
