## Project Overview

AIPP (AI 助手平台) is a cross-platform desktop application built with Tauri 2.0 — Rust backend with SQLite (rusqlite), React 19 + TypeScript + Vite frontend, shadcn/ui + Tailwind CSS v4. It lets users chat with multiple LLMs, execute scripts, manage conversations/artifacts, run scheduled AI tasks, orchestrate work through Butler, and extend functionality through MCP/Skills/Plugins, plus experimental Feishu integration.

- 详细架构（技术栈、窗口列表、前后端目录结构、API 模块、内置 MCP 工具、关键产品面）：[docs/architecture.md](./docs/architecture.md)
- 常见开发任务操作步骤（新增 API/组件/助手类型/表结构）：[docs/development-guide.md](./docs/development-guide.md)
- 测试框架与编写规范：[docs/testing-guidelines.md](./docs/testing-guidelines.md)
- 各功能用户向文档：[docs/product/](./docs/product/README.md)（ACP 集成细节见 `docs/product/11-ACP集成.md`）

**移动端**：移动端（Android）相关的开发任务必须先阅读 [AGENTS-mobile.md](./AGENTS-mobile.md)，其中包含移动端现状、检测机制、导航方案、桌面功能兼容性与改动规范。移动端优化规划见 `docs/mobile-optimization-prd.md`。

## Essential Build Commands

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
