# 测试规范

本文档从根目录 `AGENTS.md` 提取，承载测试框架、组织方式与编写规范的完整细节。`AGENTS.md` 只保留运行命令与红线。

## Technology Stack

**Backend (Rust):**
-   `#[tokio::test]` - 异步测试
-   `rstest` - 参数化测试
-   `tempfile` - 临时数据库
-   `tauri::test` - Tauri 集成测试

**Frontend (React/TypeScript):**
-   `Vitest` - 测试框架（Vite 集成）
-   `@testing-library/react` - 组件测试
-   `@testing-library/user-event` - 用户交互模拟
-   `happy-dom` - DOM 环境
-   Mock `@tauri-apps/api/core` invoke 调用

## Test Organization

> ⚠️ **重要**: 测试代码必须按功能域分离到独立文件，禁止将所有测试写在单一文件中

**测试文件命名规范：**
- 测试文件名 = 源文件名 + `_tests.rs`（后端）或 `.test.tsx`（前端）
- 例如：`conversation_db.rs` → `conversation_db_tests.rs`
- 例如：`ConversationList.tsx` → `ConversationList.test.tsx`

**后端测试 (Rust):**
```
src-tauri/src/
├── api/tests/               # API 集成测试
│   ├── mod.rs
│   ├── ai_api_tests.rs
│   ├── conversation_api_tests.rs
│   └── regenerate_tests.rs
├── db/tests/                # 数据库 CRUD 测试（模块化目录）
│   ├── mod.rs              # 测试模块入口
│   ├── test_helpers.rs     # 共享辅助函数
│   ├── conversation_db_tests.rs   # 对应 conversation_db.rs
│   ├── message_db_tests.rs        # 对应 conversation_db.rs 中的 MessageRepository
│   ├── attachment_db_tests.rs     # TODO
│   └── assistant_db_tests.rs      # TODO
└── template_engine/tests.rs  # 小模块可用单文件
```

**前端测试 (React/TypeScript):**
```
src/
├── __tests__/               # 全局测试配置
│   ├── setup.ts            # 测试环境初始化
│   └── mocks/tauri.ts      # Mock Tauri invoke
├── components/
│   └── [Component]/
│       ├── Component.tsx
│       └── Component.test.tsx  # 组件测试（同级放置）
├── hooks/
│   └── useXxx.test.ts      # Hook 测试（同级放置）
└── utils/
    └── utils.test.ts       # 工具函数测试
```

**测试文件规则:**
1. 每个功能域一个测试文件（禁止单一大文件）
2. 共享辅助函数放入 `test_helpers.rs` / `mocks/` 目录
3. 测试函数命名: `test_[功能]_[场景]` (Rust) / `should [行为] when [条件]` (TS)
4. 前端测试与源文件同级放置

## Testing Guidelines

1. **后端测试**: 使用内存 SQLite 数据库，测试 CRUD、版本管理、消息过滤逻辑
2. **前端测试**: Mock Tauri invoke，测试组件渲染、用户交互、Hook 状态管理
3. **集成测试**: 测试完整用户流程（配置修改→保存→读取）
4. **测试命名**: `test_[功能]_[场景]` (Rust) / `should [行为] when [条件]` (TS)

## Test Data Isolation (重要)

> ⚠️ **关键**: 所有后端测试必须使用内存数据库，确保不影响真实的 db 文件

**内存数据库使用:**
```rust
// ✅ 正确：使用内存数据库
let conn = Connection::open_in_memory().unwrap();

// ❌ 错误：绝对不要在测试中使用文件路径
// let conn = Connection::open("path/to/db.sqlite").unwrap();
```

**测试隔离特性:**
- 每次 `open_in_memory()` 创建独立的数据库实例
- 测试结束后自动销毁，无需清理
- 不同测试之间完全隔离，互不影响

## Test Documentation (Rust)

> Rust 没有类似 Jest 的 `describe` 块，使用文档注释描述测试用例

**注释规范:**
```rust
/// 测试消息的完整 CRUD 生命周期
///
/// 验证内容：
/// - Create: 创建消息后返回有效 ID
/// - Read: 能够根据 ID 读取完整消息信息
/// - Update: 修改消息内容后持久化成功
/// - Delete: 删除后无法再读取到该消息
#[test]
fn test_message_crud() {
    // ...
}
```

## Running Tests

```bash
# 后端测试（精确范围运行）
cargo test --manifest-path src-tauri/Cargo.toml [test_name]

# 前端测试
npm run test        # 运行所有测试
npm run test:watch  # 监听模式
npm run test:coverage  # 覆盖率报告
```
