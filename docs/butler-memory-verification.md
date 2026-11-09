# 管家长期记忆复测步骤

使用该分支构建应用，打开桌面管家中两个不同助手 A、B 的联系人对话。无需新按钮或设置。

1. 对 A 说“记住，我希望你回答简短”。保存成功后，新开 A 的管家对话询问自己的回复偏好，应能检索到。
2. 对 B 查询 A 的私有记忆，记忆接口不能读取。注意旧的跨助手历史搜索仍是另一项能力，不把它当作记忆隔离已覆盖。
3. 对 A 说“所有助手都记住，我希望使用中文回答”。B 可以检索到此全局记忆，但不能通过记忆来源接口读取 A 的私聊原文。
4. 请 A 更正自己的一条记忆，检索应返回新内容；要求忘记后应不再从记忆库检索到。完全相同的重复保存只保留一条。
5. 删除保存来源所在的聊天；已保存记忆继续存在，来源查询提示原消息已删除。
6. 请求保存群记忆应明确说明尚未开放，不能退回全局保存。

编译和自动测试使用默认 `src-tauri/target`：

```bash
npm run build
cargo check --locked --manifest-path src-tauri/Cargo.toml -j1
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib butler_memory -j1
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib mcp::butler_tools::tests -j1
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib test_cascade_delete_assistant_memory -j1
cargo build --locked --manifest-path src-tauri/Cargo.toml -j1
```

自动测试只验证工具/存储边界，不替代真实模型指令遵循和桌面端到端验收。没有调用真实付费模型。
