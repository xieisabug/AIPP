## Project Overview

AIPP (AI 助手平台) is a cross-platform desktop application built with Tauri 2.0 — Rust backend with SQLite (rusqlite), React 19 + TypeScript + Vite frontend, shadcn/ui + Tailwind CSS v4. It lets users chat with multiple LLMs, execute scripts, manage conversations/artifacts, run scheduled AI tasks, orchestrate work through Butler, and extend functionality through MCP/Skills/Plugins, plus experimental Feishu integration.

- 详细架构（技术栈、窗口列表、前后端目录结构、API 模块、内置 MCP 工具、关键产品面）：[docs/architecture.md](./docs/architecture.md)
- 常见开发任务操作步骤（新增 API/组件/助手类型/表结构）：[docs/development-guide.md](./docs/development-guide.md)
- 测试框架与编写规范：[docs/testing-guidelines.md](./docs/testing-guidelines.md)
- 各功能用户向文档：[docs/product/](./docs/product/README.md)（ACP 集成细节见 `docs/product/11-ACP集成.md`）

**桌面管家**：联系人长期私聊、总管家主会话和派发任务会话独立。每个助手只有一条当前私聊（`butler_contact`，按助手查找）；「重开新会话」把旧私聊改为 `butler_contact_archive` 归档（总管家旧主会话仍是 `butler_main`、以 `butler_main_state` 区分当前），归档会话不参与协作，但管家搜索与助手历史检索可查到并标 `archived`（`reset_butler_contact_conversation`、`db/butler_history.rs`）。谁接手工作谁和用户沟通：受托助手默认在自己的会话里直接交付/提问（`deliver_to=user`），总管家只协调，要求整体汇总时才收回（`deliver_to=sender`）。受托助手可再请协助（有界多层：parent/depth、按工作链 chain 计预算与有效期、按助手身份防循环，见 `db/butler_peer.rs`），独立任务也可发起并接收协作回送；停止会实际打断属于该工作的执行（`api/butler_peer.rs` halt_*）。引用回复/续接/任务提问见 `api/butler_work.rs`；助手引用回复是回复首行 `[[QUOTE:原话片段]]`，私聊在前端 `butlerTranscript` 解析定位、群聊在 `finish_turn` 记为 `reply_to`，两端匹配规则须一致（`utils/reply_quote.rs` ↔ `butlerReplyQuote.ts`）。助手间协作须保留发送方和请求归属；团队历史检索只覆盖管家会话。聊天正文的简洁展示与模型工作上下文分别处理。管家模式全部助手直接注入 SuperAdmin 与 assistant 查询/协作工具，不受原助手 MCP 动态加载配置影响；普通聊天不继承该权限。见 [Butler 任务系统](./docs/product/18-Butler任务系统.md)。

**管家长期记忆**：`butler_memory` 工具 + `db/butler_memory.rs` 单表，范围为助手私有 / 群（本群成员共享）/ 全局，私有记忆不进群。只有本轮用户本人的消息（群里为直接回应用户发言的回复）能写入；协作、派工、续接转达、后台续跑不算。每轮附带的 `<long_term_memory>` 随用户消息入库并逐字回放：首次按相关度在 800 字预算内附带一批（`initial="true"`，不保证是全部，放不下的提示条数、模型可 recall），之后对照“这次要发出的历史”里模型已经看到的记忆（附带过的记忆块、压缩检查点里的记忆块、本会话 `butler_memory` 工具返回的保存/更正/忘记/查到的结果）只附带新增/更新/删除与相关未看到的记忆；压缩时摘要输入去掉记忆块（`butler_memory::without_memory`），在摘要检查点后面重新首次附带一次当前版本（`CheckpointStore::memory_snapshot`），不在用户消息里整批重带。界面不显示附带了哪些记忆。删除聊天保留记忆，删群删群记忆，删助手删其私有记忆。界面在联系人资料“记忆”页与群聊信息面板。见 `api/butler_memory.rs`。

**本轮附加上下文入库**：当前时间、外部渠道/插件前缀、工作记录、引用/近期工作、长期记忆用 `<aipp_turn_context>` 接在用户消息正文末尾（附件之后）一起存储，发送内容 == 存储内容，下一轮、工具续写、重新生成都逐字回放（缓存前缀稳定、续写不丢上下文）。工作记录与近期工作和历史里最近一次相同时不重复附带。凡是把用户消息正文给人看或做检索/摘要/标题/转发的地方，都必须先去掉这块：Rust 用 `utils/turn_context.rs` 的 `visible`/`visible_sql`，前端用 `src/utils/turnContext.ts`（两端规则一致：以闭合标签结尾、从第一处“开标签+说明行”算起，用户自己打的标签不算）；界面只显示原话，这块（包括其中的记忆）一律不显示。

**管家群聊**：群聊与联系人同列，用户直接发言，不选接收人或讨论模式。谁来回复：`@成员`/`@所有人` → 引用的原作者 → 单成员 → 调度模型（小型辅助模型，未配置用总管家模型；失败如实报错不换模型）；多人同时回复（之后通过读取位置补上彼此的回复）；要求讨论时按轮推进（一轮全部结束后进入下一轮，静默轮/轮数上限/用户插话/停止即结束）；成员回复里的 @ 触发接力（有层数/次数上限）。成员说话方式（简短、分段、不逐条列点）每轮随 `<group_chat>` 提醒。每个成员在每个群有独立执行会话，输入是纯文本聊天记录（不含调度字段），私聊与其他群不注入；群成员不提供联系人协作工具。回复只在仍进行时写入成员会话，停止/移出后的迟到结果不公开，重启不重做。实现见 `db/butler_group.rs`、`api/butler_group*.rs`、`src/components/butler/ButlerGroupChat.tsx`，设计与验收见 [群聊 PRD](./docs/butler-group-prd.md)。

**聊天界面只有一套**（Ask 是单轮弹窗，只共用输入框）：
- 输入框：ChatUI、管家私聊/总管家（经 `ConversationUI`）、Ask、管家群聊都直接用 `src/components/conversation/InputArea.tsx`，输入相关新功能加在这里即全部生效；各场景差异用能力 props 表达（不传 `handleChooseFile` = 不支持附件，如群聊；`mentionCandidates` 替换 @ 候选，群聊传本群成员与“所有人”；`topChip` 引用条），不要再为某个场景手写输入框。发送语义各场景自理，但发送键约定一致：有内容即发送（回复中可插话/排队），空输入且在回复时为停止。
- 消息列表：私聊与群聊共用 `VirtuosoMessageList` + `useMessageListElements`（虚拟化、打开贴底与晚到布局跟随、发送滑动、跳转高亮、最后一轮布局）与 `useScrollManagement`、`ConversationTurnRail`；气泡统一是 `MessageItem`。群聊（`ButlerGroupTranscript.tsx`）通过 `transcriptKey`（消息分属多个成员执行会话）、`mergeAssistantMessages={false}`（不同成员不并气泡）、`renderMessageRow`（发言人/时间分隔/系统提示）、`inlineInteractionItems`（正在输入/成员提问）接入；列表层改进直接作用于两者，不要再为某个场景手写列表或滚动逻辑。

**移动端**：移动端（Android）相关的开发任务必须先阅读 [AGENTS-mobile.md](./AGENTS-mobile.md)，其中包含移动端现状、检测机制、导航方案、桌面功能兼容性与改动规范。移动端优化规划见 `docs/mobile-optimization-prd.md`。

## Essential Build Commands

**genai 依赖**：AIPP 不内嵌 genai 源码，通过 `src-tauri/Cargo.toml` 的 GitHub Git 依赖和固定 `rev` 使用独立 `xieisabug/rust-genai` 仓库。Provider/协议实现与缓存协议补丁在该独立项目维护；AIPP 保留应用配置、持久化与使用策略。更新依赖前先推送库提交，再更新 `rev` 与 `Cargo.lock`，不得提交依赖本机目录的路径配置。

```bash
# Verify frontend changes (includes TypeScript check)
npm run build

# Verify Rust backend changes
cargo build --manifest-path src-tauri/Cargo.toml

# Build complete application
npm run package

# Development mode (not recommended for debugging)
npm run dev

# Run Rust tests
cargo test --manifest-path src-tauri/Cargo.toml
```

**Important**: Frontend debugging should be done through the built application, not through `npm run dev`.

**Important**:

- Rust 编译与测试必须使用默认编译位置。禁止为了绕开占用而切换到其他编译目录、临时 `CARGO_TARGET_DIR`、或任何“另一处”构建输出路径。
- 如果发现已经有别的编译在占用默认编译位置，不要改去别处编译，直接等待占用结束后再继续。
- 不要随意运行 `cargo fmt` 或做与当前任务无关的批量格式化。只有在用户明确要求，或为修复当前改动直接导致的格式/编译问题而必须时，才进行最小范围的格式化，避免 diff 混入无关改动。

- If you add a standalone Rust binary under `src-tauri/src/bin/` (for example, a debug/preview CLI), keep `src-tauri/Cargo.toml` aligned with `default-run = "Aipp"`.
- Otherwise `cargo run` becomes ambiguous once multiple binaries exist, which breaks the default Tauri dev flow with errors like: `cargo run could not determine which binary to run`.
- Run auxiliary CLIs explicitly with `--bin`, for example: `cargo run --manifest-path src-tauri/Cargo.toml --bin feishu_markdown_debug -- --help`.

## Key Development Patterns

### Frontend-Backend Communication

```rust
// Backend: Define Tauri command
#[tauri::command]
async fn get_conversation(id: String) -> Result<Conversation, String> {
    // Implementation
}

// Frontend: Call command
import { invoke } from '@tauri-apps/api/core';
const conversation = await invoke('get_conversation', { id: conversationId });
```

### State Management

**Frontend State Management:**

```typescript
// Use domain-specific custom hooks
const { deleteConversation, listConversations } = useConversationManager();
const { models, updateModel } = useModels();
const { assistant, saveAssistant } = useAssistantRuntime();

// New hook patterns for feature management
const { formConfig } = useAssistantFormConfig(assistantType);
const { featureConfig, updateConfig } = useFeatureConfig();

// Hook naming convention: use[Domain][Action/Manager]
// Examples: useConversationEvents, useMessageProcessing, useFileManagement
```

**Backend State Management:**

```rust
// Thread-safe state with Arc<TokioMutex<T>>
struct FeatureConfigState {
    configs: Arc<TokioMutex<Vec<FeatureConfig>>>,
    config_feature_map: Arc<TokioMutex<HashMap<String, HashMap<String, FeatureConfig>>>>,
}

// Always use async-aware locks
let config = state.configs.lock().await;
```

### Component Patterns

-   Prefer shadcn/ui components from `@/components/ui`
-   Use Radix UI primitives for complex interactions
-   Follow existing component structure and naming conventions
-   Keep complex logic in Rust, UI logic in React
-   Use domain-specific component organization (config/, conversation/, etc.)
-   **Icon 样式规范**：
    -   使用 `lucide-react` 图标组件
    -   图标尺寸使用 `w-full h-full` 或指定尺寸如 `h-4 w-4`
    -   **在 Button/IconButton 内**：通常不需要指定颜色类（由组件继承）
        -   示例：`<Plus className="h-4 w-4" />`（在 Button 内）
        -   示例：`<Edit2 size={16} className="text-icon" />`（在 IconButton 内）
    -   **在自定义容器内**：需要指定颜色类 `text-muted-foreground`
        -   示例：`<ServerCrash className="w-full h-full text-muted-foreground" />`（侧边栏菜单项）

## Development Guidelines

1. **Cross-Platform**: Ensure compatibility across Windows, macOS, and Linux
2. **Performance**: Optimize resource loading, use caching, minimize re-renders
3. **Async Operations**: No blocking operations in Rust, use Tokio runtime
4. **Type Safety**: Maintain TypeScript strict mode and Rust type safety
5. **Error Handling**: Provide meaningful error messages to users
6. **Testing**: Write tests for new functionality, especially AI-related features
7. **Code Organization**: Follow domain-driven structure for both frontend and backend
8. **No Model Fallback**: 当用户配置的模型（如对话总结模型、助手模型等）在数据库中不存在时，禁止自动回退到其他模型。应该直接返回错误信息，在界面上提示用户检查配置，而不是随意选择其他模型执行任务
9. **工作日志**：干活的时候边干活边写工作日志，只要有一定的成果了就要更新一下，方便后续交接。日志文件放在 `tmp/worklog/` 目录下（按主题/分支命名，如 `mobile-optimization.md`），记录：做了什么（文件、关键改动）、验证结果、未完成事项与下一步。

## Failure and Fallback Policy

- **禁止擅自降级**：Codex 必须始终使用 Codex 原生通道，Claude Code 必须始终使用 Claude Code 原生通道。启动、恢复、模型、MCP、权限、进程或协议任一环节失败时，必须立即报错并停止；禁止自动切换到 ACP、其他模型、其他 provider、新会话、无工具模式或任何能力缩减路径。只有用户针对当前需求明确要求某种降级方案时才允许实现。
- **错误必须具体且可追溯**：禁止只返回“运行失败”“会话已停止”“未知错误”等笼统文案。用户可见错误与持久化错误必须包含原始失败环节和底层原因；进程型通道还应捕获并记录 stderr、退出状态或协议错误，并附带可关联的 conversation/session/run 标识。若底层没有提供原因，必须明确指出缺失的是哪一层诊断信息以及为何缺失，不能用泛化文案覆盖。

## Testing Changes

Always verify both frontend and backend changes:

```bash
# Check TypeScript
npm run build

# Check Rust
cargo check --manifest-path src-tauri/Cargo.toml

# Run Rust tests，When running Rust tests, please run them with precise, minimal scope—for example, by method or by file.
cargo test --manifest-path src-tauri/Cargo.toml [test_name]
```

红线与规范（完整版见 [docs/testing-guidelines.md](./docs/testing-guidelines.md)）：

- 所有后端测试必须使用**内存数据库**（`Connection::open_in_memory()`），禁止在测试中使用文件路径
- 测试按功能域分文件：源文件名 + `_tests.rs`（后端）/ `.test.tsx`（前端），前端测试与源文件同级放置
- 测试函数命名: `test_[功能]_[场景]` (Rust) / `should [行为] when [条件]` (TS)

### Validation runner note

- In this environment, `powershell` **sync-mode** is unreliable for long-running validation commands: even a single `cargo`/`npm` build or test can finish in the child process while the parent `pwsh` stays open, making the shell look hung and hiding completion/output.
- For repository validation (`cargo build`, `cargo test`, `npm run build`, `npm run test`), treat **async mode with explicit `read_powershell` output reads as mandatory**. Do not use sync mode for these commands.
- Run **exactly one validation command per shell**. Do not chain validation commands with `&&` or start the next validation before the previous shell has been fully read/stopped.
- When launching async validation, append an explicit completion marker such as `; Write-Host "__AIPP_DONE__:$LASTEXITCODE"` and keep reading until that marker appears. Do not infer completion from the `pwsh` process state alone, because async sessions stay alive after the child process exits.
- If a validation shell appears stuck and no `cargo`/`npm` child process remains, stop that shell and rerun with an explicit completion marker.

### Chat scroll perf reproduction

长对话滚动卡顿/跳动问题的复现与调试，统一走内置 ChatUIWindow 自跑 harness（`AIPP_CHAT_SCROLL_PERF_*` 环境变量），完整方法、命令与结果字段说明见 [docs/chat-scroll-perf-playbook.md](./docs/chat-scroll-perf-playbook.md)。

## Documentation Sync Guidelines

- When a user-facing feature changes, update the matching file under `docs/product/` in the same task when practical.
- Keep `docs/product/README.md` aligned with the actual set of product docs and major user-visible features.
- 架构、开发任务、测试规范的细节分别维护在 `docs/architecture.md`、`docs/development-guide.md`、`docs/testing-guidelines.md`；改动对应内容时同步更新这些文件，`AGENTS.md` 只保留规范红线与索引。
- Keep this `AGENTS.md` aligned with major architecture/product-surface changes that affect future engineering tasks, especially Butler, Feishu, export formats, scheduling model, and Skills behavior.
