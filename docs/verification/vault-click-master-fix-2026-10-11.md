# 密码库默认窗口与主密码换密修复（2026-10-11）

本轮承接用户已批准的密码库精简方案，只修复默认窗口交互与偶发换密，保留进程持续解锁。实际测试固定使用 `--vault-fictional`，不初始化正式存储、不连接 QQ。正式准入与发布结论仍为 BLOCKED。

## 修复与失败回归

1. **视口滚动。** 旧 `.vault-shell` 只有最小高度，长列表拉长整个文档；默认窗口曾测得文档高 2405 px、根滚动 1357.33 px，窄窗口失败回归的根滚动为 571.33 px。改为 `height:100dvh; overflow:hidden`，内容区 `min-height:0; overflow:auto`，标题操作区和状态区不收缩。回归检查根滚动为 0、内容确实滚动、锁定按钮完整位于视口且命中正确元素，并用普通指针完成锁定和编辑。此前间歇性 Playwright `html intercepts pointer / outside viewport` 并未单独稳定复现；此处关闭的是已复现的视口漂移及最终流程中的点击回归，不将其强行归结为单一历史原因。
2. **点击触发意外锁定。** 完整桌面失败轮实际出现 `CONFLICT`，界面变为“已锁定”。同一点击的 pointerdown 会发送遗留 `vault_activity`，按钮 click 再发送真实操作；它们及切页的 list 可能竞争容量为 1 的原生队列，队列冲突经原有错误处理触发锁定。新增受控 pointerdown→click 回归在旧实现确定失败（期望已解锁、实际已锁定），修复后通过。删除已经取消闲置过期后不再需要的 pointer/keyboard 心跳；真实业务错误继续锁定，原生队列容量、Activity API、权限及生命周期处理保持。
3. **换密内存分配。** 旧 hash-wasm 每次 KDF 创建新的 WASM 内存，64 MiB 工作区依赖不确定的 GC 回收。同一 helper 连续编辑、换密时，在原有 512 MiB Job 限制内发生 `RangeError`，出现在验证新密码或拒绝旧密码阶段，对外仍是 AUTH_FAILED。用固定随包 Node 26.11.1 的原生 `crypto.argon2Sync('argon2id', ...)` 替换实现；每次工作区随原生调用释放，输出复制后清零。Argon2id v19、盐/输出长度、内存/迭代/并行度边界、KDBX 文件格式保持，未提高 Job 512 MiB 或 V8 heap 128 MiB 限制。

原生回归 `long_lived_helper_changes_master_after_edits_with_existing_memory_limit`：在同一 helper 中连续 8 次元数据编辑和主密码切换，验证条目密码保留、重开和旧密码拒绝；旧实现失败，修复后通过。标准 Argon2id 向量、现有虚构旧库和真实加密重开通过。

## 最终本机结果

最终真实 Windows WebView2 密码库流程 **28 项通过，complete=true、直接 exit 0**。其中包含 20 次显示请求立即切页的回归；实际闲置 **310003 ms** 后，原生列表和 UI 仍解锁，随后普通点击锁定、重新解锁和删除结果检查通过。默认、窄窗口及窄编辑表单均通过内容滚动和操作区命中检查；未使用最大化绕过。

前端 193 passed；helper 4 passed；类型、前端构建、修改文件格式、Rust 格式、全工作区严格 Clippy 和 Windows custom-protocol 桌面构建通过。最终完整 Rust 为 507 passed / 21 failed / 15 ignored，失败仍仅为原有附件目标 `dpapi_restore`、`image_pdf`、`parser_isolation`，不能称全工作区全绿。

新 KDF 下实际 Windows 原生边界 33 类通过，4 类仍阻断：第二真实身份、物理断电、真实磁盘满、完整 reparse 夹具权限。源/桌面运行资源各 198 项集合与 SHA256 完全一致；两个平台 manifest 更新同一 crypto 哈希。Linux 资源树本机不存在，本轮不授予 Linux 运行验收。

## 既定行为与覆盖范围

- 切模块、最小化、关窗到托盘、真实闲置超过 5 分钟保持解锁；手动锁定和整个进程重启需主密码。
- 密码显示 15 秒自动遮罩，切页丢弃临时显示；剪贴板不自动清理，锁定和退出仍保留已复制的虚构值。
- 元数据编辑不输入密码仍保留原 ProtectedValue；按需修改、取消修改、必填校验和显式替换通过；删除确认、创建时不可找回提醒、短提示约 2.5 秒消失保持。
- 实际 WebView2 页面、双向 IPC 拒绝、真实 helper/KDBX、系统剪贴板、原生最小化和 WM_CLOSE、实际进程终止重启均参与；没有 mocked IPC/crypto、force click、最大化绕过或加速闲置时钟。
- 已核对默认及窄窗口的 vault WebView 截图布局和遮罩。原生前台组合窗口截图、托盘菜单退出、跨屏 DPI、安装卸载、剪贴板历史/云同步没有在本轮获得验收。WM_CLOSE 后用原生恢复窗口，进程重启用终止本次拥有的测试子进程，不能替代托盘菜单退出。锁屏/睡眠保持用户取消的范围外。

## 产物与证据

- HEAD：`a76f4ba619ad610c040acff857ba47a558e8d86c`，分支 `codex/shixu-v0.1`；实现保留在未提交工作区。
- 最终 EXE SHA256：`db084f7bb7211ce9233d6176121f8ad3631ff97f5a938c5b9c956de49247bf1f`。
- 编译时源码 SHA256：`eeba8e1c02dcc37723bd529c0f2cf139e33469f471f5488b7bb03b3ccaab2981`；之后仅新增本报告/档案，业务代码与 audit.json 记录一致。
- 测试拥有的进程已退出；最终虚构库 fixture B，仅指脚本内固定公开测试常量。原有库不重置，成功轮新增条目已删除。

证据目录：`docs/verification/local/vault-fix-20261011/`（忽略目录，不提交虚构 KDBX/日志/截图）。

- `baseline/`：本轮接手源文件和旧 EXE；原有脏工作区保留。
- `memory-red.log`、`memory-green.log`：原生内存红绿回归；`stress-kdf-green/result.json`：8 轮编辑后的 16 次主密码切换通过。
- `viewport-red/result.json`：旧布局失败；最终桌面结果的 `viewportChecks` 保存默认、窄窗口及窄编辑表单的真实尺寸/滚动/命中。
- `activity-red.log`、`activity-green.log`：旧遗留心跳引起锁定的确定性红绿回归。
- `desktop-final/`：显示请求后立即切页的失败；`switch-control/`：4 轮截图/无截图、立即/等待显示对照；`desktop-final2/`：测试故障注入早于初始化就绪；`desktop-final3/`：真实 CONFLICT 后锁定。失败证据全部保留。
- `desktop-final4/`：最终完整桌面 JSON、编译身份及截图；`desktop-final4.log`、`desktop-final4-build.log`：直接输出。
- `frontend-final.log`、`helper-green.log`、`rust-final.log`、`native-boundary-final.log`、`audit.json`：各层结果和源码/资源校验。
- `release-final4.json`：最终 EXE 实际发布校验，exit 2、12 项 BLOCKED；不是生产接入或发布授权。

完整原生边界通过直接执行固定 `synthetic_native_boundary` ignored test 取得。本轮未运行要求 clean committed tree 的原封不动 boundary/minimum runner，不以局部结果冒充该固定 runner。

最终独立审阅未发现必须修复问题，已核对资源、算法边界、临时诊断清除、布局和取消遗留心跳；审核不代替实机验收。

本轮未提交、推送、安装或发布。旧资源快照、原库及失败轮虚构条目保留；成功完整轮只删除该轮唯一新增条目。正式构造器仍 Unsupported，外部准入条件继续保留。
