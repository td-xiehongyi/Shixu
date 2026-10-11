# Shixu Main-Window Vault Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans for native execution, or superpowers:subagent-driven-development if the user explicitly selects delegation. Steps use checkbox (`- [x]`) syntax for tracking. Execution: Native, explicitly selected by user.

**Goal:** 按用户选定的图一/图三把密码库放进拾序主窗口，单次进程运行解锁一次后保持可访问。

**Architecture:** 一个 Windows 原生窗口承载 main 与 vault 两个隔离 WebView。main 保留导航与日历/通知/设置，vault 在导航右侧承载锁定、列表详情及页内表单；正常离开只隐藏视图。会话保留到手动锁定或完全退出，原生撤销与故障隔离继续作为交付依据。

**Tech Stack:** 现有 React/TypeScript/Vitest、Rust、固定 Tauri 2.12.2 + unstable feature、Windows WebView2、既有 KdbxWeb helper；不升级依赖。

**Spec:** [已确认设计](C:/Users/xie/Desktop/Shixu/docs/superpowers/specs/2026-10-11-vault-main-interface-design.md)（2026-10-11 用户回复“确认”）。

## Global Constraints

- 采用图一的列表与详情布局、图三的页内解锁布局；不采用图二作为视觉参考。
- 保留当前导航宽度 214px；密码明文显示 15 秒后遮罩。
- 切换到日历、通知或设置、最小化、关窗到托盘均保持解锁；取消 5 分钟闲置自动锁定。
- 完全退出或进程重启后必须重新输入主密码，不跨启动持久化解锁状态、主密码或密钥。
- main 与 vault 按实际 WebView 绑定权限，原生校验调用标签与本地 origin。主导航只接收锁定状态。
- 保持旧条目/多行描述、密码换行拒绝、剪贴板不自动清理、系统锁屏/睡眠钩子范围外。
- 正式存储准入及 12 项发布门槛保持阻断，只用专用虚构密码库作桌面验收。
- 保留已有未提交改动、KDBX、资源、旧 EXE 和旧证据；不提交、推送、安装或发布。在现有工作区增量实施，不通过创建干净 worktree 丢掉既有描述字段工作。

## Review Focus

1. 快速连续切页、重复点击入口、晚完成的原生视图创建：最终页面以最新导航请求为准（Task 2/3）。
2. 隐藏期间密码显示/保存/解锁的迟到结果：清除明文，允许已经提交的保存完成，不覆盖手动锁定、不重新弹出视图（Task 2/3）。
3. 前端/原生/核心某一层仍按 300 秒失效：超过 5 分钟真实桌面与三层定向测试均能访问（Task 1/3/4）。
4. 同原生窗口下 capability 意外合并：测试两 WebView 双向拒绝、远程与伪造调用（Task 2/4）。
5. DPI/最大化/760px 窄窗口、长账号/描述、搜索过滤掉选中项：导航可点、按钮可见、旧详情不残留（Task 2/3/4）。

---

### Task 1: 单次进程会话持续解锁

**Files:** Modify `crates/shixu-core/src/vault/{session,service}.rs`, `crates/shixu-core/tests/vault_policy.rs`, `src-tauri/src/{vault,lifecycle}.rs`, `src-tauri/tests/windows_lifecycle.rs`.

**Interfaces:** 保持 `VaultService::tick(now: UtcMillis) -> AppResult<()>`、`activity(&SessionId, now)` 和 `VaultController::{tick,lock,shutdown}` 的原有签名；新增 `VaultController::is_unlocked(&self) -> AppResult<bool>`，只查询当前有效授权，不泄露 SessionId。

- [x] 先检查本机 `node --version`、`pnpm --version`、`cargo --version`，检查固定 Rust/MSVC 环境；保存原工作区状态与本次受影响源码哈希到忽略的 `docs/verification/local/vault-main-interface-2026-10-11/`。备份旧 EXE 到该忽略目录后再构建，不覆盖旧证据。
- [x] 写红测：core `session_survives_idle_until_explicit_lock` 在 300001 和 86400000 毫秒仍可 list；native `native_session_survives_idle_without_activity` 在对应旧活动时间后仍有效；`window_close_hides_without_revoking_vault` 保持 session，`exit_revokes_vault` 和旧 session 迟到结果仍拒绝。反向时钟仍返回 InvalidInput。
- [x] 运行 `cargo test -p shixu-core --test vault_policy --target x86_64-pc-windows-msvc --locked --offline`、`cargo test -p shixu-desktop --lib --test windows_lifecycle --target x86_64-pc-windows-msvc --locked --offline`，记录预期失败。
- [x] 删除核心时间观察中的闲置失效条件及原生 Authority 的 300 秒过期逻辑，保持单调时间检查、身份/epoch/取消与错误锁定；调整旧闲置用例为新契约，避免删除仍有效的撤销/时钟断言。`LifecycleEvent::WindowClose` 只隐藏，Exit 继续锁定和停止服务；实际 runtime 关窗通知在 Task 2 同步。
- [x] 实现 `is_unlocked`：同步原生 signal 代际后读取授权；停机/故障不得报告 true。跑上述定向套件至通过，保留未实现 Task 2 导致的桌面接线限制。

### Task 2: 单窗口内的隔离密码视图

**Files:** Create `src-tauri/src/vault_view.rs`, `src-tauri/tests/vault_view.rs`; Modify `src-tauri/src/{lib,runtime,commands}.rs`, `src-tauri/build.rs`, `src-tauri/Cargo.toml`, `src-tauri/capabilities/{main,vault}.json`, `src-tauri/tests/ipc_permissions.rs`, `src/contracts/{bridge,bridge.test}.ts`.

**Interfaces:**
- main bridge 新增 `setVaultViewVisible(visible: boolean, revision: string): Promise<void>`、`getVaultLockState(): Promise<boolean>`；用前者替代 `showVaultWindow`。
- 原生命令 `set_vault_view_visible` payload 严格为 `{visible, revision}`；revision 复用规范十进制 u64 校验，最新导航请求胜出。`get_vault_lock_state` 无参数，返回 Task 1 的有效授权布尔值。
- 原生 `VaultViewHost::set_visible(&self, app: &tauri::AppHandle, visible: bool, revision: u64) -> AppResult<()>`；`resize(&self, app: &tauri::AppHandle) -> AppResult<()>`；`redact_hidden(&self, app: &tauri::AppHandle) -> AppResult<()>`。Windows 方法受 cfg 控制；可移植的 revision/尺寸策略可在普通 Rust 单元测试运行。
- 定向 vault WebView 事件：`vault_locked`（空载荷）、`vault_view_visibility`（布尔值）；定向 main 的 `vault_lock_state`（布尔值）。失败状态通过固定错误码返回，不含条目数据。

- [x] 写红测：新命令 main-only，所有 vault 操作仍 vault-only；vault 拒绝日历/QQ；伪造标签/远程 origin/多余参数/非规范 revision 被拒绝。按 webviews 精确绑定 capabilities，不能留下把两个视图覆盖的 windows 匹配。
- [x] 写 host 策略红测：show(1) 后 hide(2) 的迟到创建不得显示；连续 show 只创建一个 vault；214px 左边界，窗口缩放/DPI 转换保持非负尺寸；主窗口隐藏不能触发 WindowClosed 撤销。
- [x] 跑 `cargo test -p shixu-desktop --test ipc_permissions --test vault_view --target x86_64-pc-windows-msvc --locked --offline` 和 `pnpm exec vitest run src/contracts/bridge.test.ts`，记录预期失败。
- [x] 启用固定 Tauri 的 unstable，使用 WindowBuilder/Window::add_child 管理 main/vault；把 NativeDesktop 与托盘路径改为获取原生 window，不依赖 WebviewWindow 单视图假设。原生创建脱离同步 WebView 回调；序列化 host 生命周期，创建完成前后检查最新 revision。
- [x] vault 只覆盖 x=214 到右边缘；原生处理 Resized/ScaleFactorChanged，避免按比例自动缩放把导航宽度一起放大。视图只加载固定本地入口，沿用 ObservationRoot 数据目录和导航限制。
- [x] 隐藏只调用 WebView hide，保留实例和服务；发布 visibility=false 清理瞬时秘密。关窗到托盘先清理瞬时显示，再隐藏主窗口；实际销毁/退出才撤销会话。事件发布、创建/尺寸错误按设计锁定并报告，不自动重试敏感操作。
- [x] 接好新命令和主导航锁定状态查询/事件，仍在现有授权交付保护内发布；state 推送在锁域外调度以避免重入死锁。跑红测至通过，Windows 编译确认固定 API 实际可用；不把编译通过写作桌面验收。

### Task 3: 图一/图三界面与页内操作

**Files:** Create `src/features/vault/VaultEntries.tsx`, `src/App.test.tsx`; Modify `src/App.tsx`, `src/features/vault/{VaultWindow,VaultWindow.test,EntryForm}.tsx`, `src/features/vault/vault.css`, `src/features/settings/SettingsPage.tsx`，以及受影响的浏览器夹具 `src/test/vault*`。

**Interfaces:**
- `VaultEntries` 接收 `rows: VaultSummary[]`, `selectedId: string | null`, `search: string`, `busy: boolean`, `visiblePassword: string | null` 和 `onSearch/onSelect/onNew/onEdit/onDelete/onReveal/onCopy` 回调。只负责列表与详情；原生调用、字节所有权及表单状态仍由 VaultWindow 管理。
- 为测试注入扩展 `VaultWindow` 的订阅参数：`subscribeVisibility?: (callback: (visible: boolean) => void) => Promise<() => void>`；默认订阅 Task 2 的 vault 事件，测试夹具不得进入产品路径。
- App 各导航统一递增十进制 revision，调用 Task 2 的可见性命令。正常切页不卸载密码子 WebView。锁定状态由原生布尔值决定；订阅失败显示不可用，不写死“已锁定”。

- [x] 写红测：主导航不打开独立窗口、最新切页胜出；解锁一次后 hide/show 不再调用 unlock、不触发 vaultLock；前端 fake timers 超过 300001ms 仍可访问；手动锁定后仍显示图三。
- [x] 写界面红测：图一搜索/新增同排、仅所选详情显示账号/换行描述、过滤掉当前条目不残留旧详情、无描述“暂无描述”；显示/复制操作绑定所选 ID；切换条目和 visibility=false 立即清除密码字节，15 秒遮罩仍有效。
- [x] 写表单/竞态红测：新增编辑和删除确认在详情区；切离清空未保存输入；正在保存完成可更新记录但不能自动显示视图；迟到 reveal 字节被清零，手动锁定后的解锁/列表返回不恢复 UI；长字段内容不被裁剪。
- [x] 跑 `pnpm exec vitest run src/App.test.tsx src/features/vault/VaultWindow.test.tsx`，记录预期失败。
- [x] 实现图一列表/详情和图三锁定区；新增按钮保留搜索右侧，详情右上编辑、字段旁复制/显示、左下删除。取消 300 秒前端计时器，保留 15 秒 reveal timer。以独立显示代际处理隐藏清理，不能用正常隐藏去撤销会话；真正卸载与错误仍清理/锁定。
- [x] 接入 Task 2 订阅，重显时用有效会话重新核对列表；Locked 返回进入图三而不循环自动解锁。设置文案改为“本次运行解锁后保持可访问；手动锁定或完全退出后需重新解锁”。760px 窗口允许内容区合理滚动，导航与按钮始终可操作。
- [x] 跑定向套件、`pnpm test`、`pnpm typecheck`、`pnpm build`，确保通过；格式检查只修本次触及文件。

### Task 4: 实际 Windows 验收与交付

**Files:** Modify `scripts/test-windows-vault-desktop.cjs`; Create `docs/verification/vault-main-interface-2026-10-11.md`；证据仅存忽略目录 `docs/verification/local/vault-main-interface-2026-10-11/`。

- [x] 修改桌面脚本使用 Task 2 的同窗口两个 WebView，独立窗口数量必须为 1；继续核实 CDP 进程归属和虚构入口。写先失败的验收断言：切页与托盘重开无需解锁、闲置 310 秒后原生 list 成功、手动锁定/重启必须重新解锁、双向 IPC 拒绝。保留四字段增删改查/换密/错密/复制与旧库兼容断言，输出目录改为本轮专用目录。
- [x] 准备已有 VS DevShell/MSVC 环境，执行 `cargo build -p shixu-desktop --target x86_64-pc-windows-msvc --locked --offline --features custom-protocol`；使用已有 Playwright 模块路径，通过 `node scripts/test-windows-vault-desktop.cjs` 验收。只终止测试拥有的进程，保留测试前 KDBX 和资源，不触碰正式数据/真实账号。
- [x] 截图核对默认窗口、最大化、760px 窄窗口以及可用 DPI 情况；检查主导航可点击、表单按钮可见、描述长文本、图一按钮位置与图三页内解锁。拿不到的 DPI/托盘交互证据明确列为未验证，不用程序终止替代托盘退出验收。
- [x] 最终源码完成后运行 `cargo test --workspace --target x86_64-pc-windows-msvc --locked --offline --no-fail-fast`、`cargo clippy --workspace --all-targets --target x86_64-pc-windows-msvc --locked --offline -- -D warnings`、`cargo fmt --all -- --check` 及 Task 3 前端检查。按最终修改范围补定向失败修复；已有附件失败单独保留并报告。
- [x] 绑定最终 EXE/源码与资源哈希，执行现有 `--verify-release` 校验，12 项阻断不得因 UI 变更假称通过。报告逐项区分自动测试、真实桌面和未验证结果；停止测试进程。
- [x] 最小更新 Obsidian Shixu 档案/索引，保存并回读；向用户交付可启动入口、截图、验收结论和剩余限制。产品尚有未验证事项时不提示归档。

## Self-review / Handoff

2026-10-11 静态自审：已覆盖图一/图三、页内四字段操作、三层持续会话、单窗口隔离、托盘/切页、迟到结果、尺寸与实际桌面；新接口定义集中在 Task 1–3，后续任务引用相同命名。运行与验收均未执行，复选框保持未完成。已有附件失败和正式准入不混作本次通过项。

建议 Native：四项工作依赖同一会话、视图与 UI 接线，留在当前聊天逐项实施更便于保留现有未提交成果。计划审阅后由用户选择 Native 或 Subagent-driven；选择委派前不生成子代理。所有路径均不包含提交/推送/发布。
2026-10-11 执行收尾：四任务已实施并交付当前范围；实际桌面 23 passed、真实 310009ms 持续解锁，前端 186 passed；Rust 501/21/15、已知附件失败保留。无提交/推送/安装/发布；跨屏 DPI/托盘菜单及正式准入仍开放，详情见本轮验收报告。
