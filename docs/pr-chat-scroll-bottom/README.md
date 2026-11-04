# PR #29 真实应用截图

- `app-long-chat-at-bottom.png`：在 Linux box 上用 **真实 Tauri 桌面端**（`src-tauri/target/debug/Aipp`，分支 `fix/chat-scroll-bottom-on-open` @ ba295dd + 重建后的前端）打开含 81 条消息的会话「长对话贴底验证 PR29」后的窗口截图。可见侧栏 + 消息列表，视口停在最后一轮（`助手回复 #40` / `END_MARKER_40`），滚动条在底部。
- `harness.html` / `harness-after-growth.png`：仅作延迟增高对照，**不是**主证据。

## 复现步骤摘要

1. `pnpm/npm run build` 刷新 `dist/`
2. `PATH="$HOME/.cargo/bin:$PATH" cargo build --manifest-path src-tauri/Cargo.toml`
3. `HOME=/tmp/aipp-client-a` + `DISPLAY=:4` 启动 debug 二进制；`feature_config.experimental.default_home_window=chat_ui`
4. 在 `conversation.db` 中写入长会话（81 msgs）后，在侧栏点开该会话
5. `scrot -a …` / ImageMagick 截取真实窗口
