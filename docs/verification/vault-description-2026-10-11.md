# 密码库描述字段（2026-10-11）

用户批准增加可选、多行“描述”。本轮范围为现有 Windows 虚构密码库；正式密码库与发布准入保持关闭。前一阶段未提交改动完整保留，没有提交、推送、安装或发布。

## 行为与兼容

- 新增、编辑条目可填写描述，也可留空；保留空格、Unicode 与换行。编辑未修改的既有描述时，保留原 CR/CRLF。写入限 4096 UTF-8 字节，超限拒绝，不截断。
- 解锁列表显示描述；锁定或关闭编辑表单时卸载并清理描述控件的 value、defaultValue 和 textContent。空描述不占列表区域。
- 描述存入 KDBX 标准 Notes 字段，使用 ProtectedValue；保存后重新解密核对摘要与密码。旧条目缺少 Notes 默认为空，不迁移或重建旧 KDBX。
- 固定 helper 增加字段后，同步 Windows/Linux 清单中唯一变化的 helper 哈希；其余固定运行资源未修改。Windows 持久根使用清单 SHA256 命名的只读资源快照，独占初始化 lease 内准备并验证；旧快照保留，已有 KDBX 字节不改写。资源准入 marker 与活动库 marker 分离，已发布快照损坏仍拒绝使用。
- 正式构造器仍 Unsupported。剪贴板保留策略不变；系统锁屏、睡眠/唤醒仍在用户取消的范围外。

## 证据

原始本机日志、截图、旧 EXE 与产物指纹：`docs/verification/local/vault-description-2026-10-11/`（Git 忽略）。

| 检查 | 结果 |
| --- | --- |
| 真实 helper + 加密 KDBX | 通过：旧 Notes 缺失、多行创建、ProtectedValue、重开、编辑、清空、原账号保留 |
| 前端全套、类型、构建 | 89 项通过；类型检查与构建通过 |
| 描述卸载清理 | StrictMode 与 detached DOM 的三种文本属性断言通过；先红后绿 |
| Rust 完整工作区（MSVC custom-protocol） | 495 passed / 21 failed / 15 ignored |
| 新增原生资源升级与保存重开回归 | 通过；旧快照保留、KDBX 字节一致并可解锁、多行描述保存重开 |
| 严格全工作区 Clippy、Rust 格式、修改前端文件格式 | 通过 |
| 实际 Windows WebView2 桌面 | 18 项通过、exit 0；描述新增、编辑、锁定隐藏、重启读取、清空及真实闲置 299946 ms 锁定 |
| 显式原生隔离边界 | 33 项通过 / 4 项既有阻断；实际当前 helper 与 OS 边界复验 |
| 发布 CLI | exit 2，12 项 BLOCKED |

完整工作区的 21 项失败仍为 `dpapi_restore` 的 1 项、`parser_isolation` 的 15 项、`image_pdf` 的 5 项：非 Unix 附件缓存 Unsupported 或缺固定引擎/模型。本轮未更改这些模块，不称全套通过。Linux 清单同步不等于 Linux 本机运行验收。

实际桌面测试使用真实 EXE、原生 AppContainer helper、系统剪贴板和持久虚构 KDBX，没有 mock IPC 或替换计时。关窗为原生 WM_CLOSE，进程重启只终止本测试拥有的进程；未代替托盘菜单退出或安装验收。helper 功能测试单独注明不是 OS 隔离证明。

最终运行代码 EXE SHA256：`76d2becf00ddc0afd9199e9c5d73532d69f8f481d211077bc62b55a4d967b813`。

构建时源码 SHA256：`ff8cf397bce1ac6e4d895421547f44856d7483aaa13458d8cdb951fb917523c4`。构建后仅补充说明与验收记录，运行代码单文件身份另存本轮忽略证据目录。

## 使用

双击根目录 `虚构密码库验收.cmd` → 密码库 → 新增条目/编辑 → 描述。仅使用虚构数据。已有专用虚构库可直接打开，旧版资源保留在原目录。前一阶段历史证据见 [Windows 虚构闭环报告](windows-desktop-fictional-2026-10-11.md)。

本机留存库现在为脚本 fixture B，虚构主密码 `Fictional-Desktop-Master-2026-B`；本轮测试新增条目已删除，测试进程已退出。它不是真实凭据。旧 EXE 保存在本轮忽略证据目录的 `previous-desktop.exe`。

下一步仍为正式存储准入、剩余 Windows/托盘退出验收、附件缓存与固定引擎、真实 QQ 和安装包；本轮描述字段不关闭这些项目门槛。
