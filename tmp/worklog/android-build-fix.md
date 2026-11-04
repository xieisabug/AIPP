# Android 目标编译修复（optimize/mobile 分支）

日期：2026-08-15

## 背景

`aarch64-linux-android` 目标编译失败，172 个错误（main 分支既有腐化，已对照验证）。原始日志：`tmp/android-build.log`。全部错误均因 desktop-only 依赖/功能缺少 `cfg(desktop)` 门控。

## 错误分组与修法

### 1. chromiumoxide 搜索模块（约 151 个错误：E0432/E0433 + 117 个 E0282 级联 + 2 个 E0308）

chromiumoxide 是 `cfg(desktop)` target 依赖，但 `search/mod.rs` 的 `pub mod chromiumoxide` 未门控。

- `src-tauri/src/mcp/builtin_mcp/search/mod.rs`：`pub mod chromiumoxide;` 与 `pub use chromiumoxide::{...}` 加 `#[cfg(desktop)]`。
- `src-tauri/src/mcp/builtin_mcp/search/handler.rs`：
  - chromiumoxide 相关 import、`GLOBAL_BROWSER_POOL`、`cleanup_search_profile_locks`、`shutdown_search_browser_pool`、`resolve_search_user_data_dir`、`is_timeout_like`、`get_or_create_browser_pool`、`fetch_search_html`、`process_html_by_type`、`load_search_config`、`build_fetch_config`、`build_general_fetch_config`、`load_search_config_from_db` 全部加 `#[cfg(desktop)]`。
  - 用户可见入口 `search_web_with_type` / `fetch_url_with_type` 增加 `#[cfg(mobile)]` stub，返回 `crate::api::system_api::mobile_unsupported_error("浏览器搜索"/"浏览器抓取")`。`SearchHandler::new` 与 types 导入保持双平台可用，`builtin_mcp/mod.rs` 无需改动。
- `src-tauri/src/mcp/builtin_mcp/search/chromiumoxide/fetcher.rs:1609/1808`：E0308 真实 bug（传 `String` 给 `&str` 参数 `save_debug_html`），改为传 `&html`。
- `src-tauri/src/lib.rs`：`cleanup_search_profile_locks` 启动调用与退出时 `shutdown_search_browser_pool` 调用点包 `#[cfg(desktop)]`。

### 2. tauri_plugin_updater / tauri_plugin_autostart 注册（lib.rs:657/663-666）

- `src-tauri/src/lib.rs`：Builder 链拆成 `let builder = ...`（公共插件）+ `#[cfg(desktop)] let builder = builder.plugin(updater).plugin(autostart)` + `let app = builder.setup(...)`。updater_api.rs / system_api.rs 中对这两个插件的使用原本就已有 cfg 门控，无需改动。

### 3. create_chat_ui_window（lib.rs:861 E0425）

定义在 `window.rs:333`，无双平台门控，只是 lib.rs 的 `use crate::window::{...}` 列表漏了它。修法：新增 `#[cfg(mobile)] use crate::window::create_chat_ui_window;`（桌面端 lib.rs 不用裸名，避免桌面 unused import）。

### 4. copilot LSP 命令（lib.rs:921、1122-1128 宏找不到）

命令定义在 `copilot_lsp.rs`，import 处（lib.rs:60-64）本就有 `#[cfg(desktop)]`，但 `generate_handler!` 条目未门控。修法：给 7 个条目（stop_copilot_lsp / check_copilot_status / sign_in_initiate / sign_in_confirm / sign_out_copilot / get_copilot_lsp_status / get_copilot_oauth_token_from_config）逐个加 `#[cfg(desktop)]`（tauri-macros 2.x 的 generate_handler 支持条目级外层属性，已核实源码）。桌面行为零变化。

### 5. build.rs 主机/目标平台混淆导致的链接错误（修复 172 个编译错误后新暴露）

`build.rs` 用 `#[cfg(windows)]` 判断是否嵌入 Windows app manifest，但 build.rs 编译运行在主机上，Windows 主机交叉编译 Android 时该 cfg 仍为真，`embed_resource::compile_for_everything` 产出 Windows COFF `aipp-app-manifest.lib` 并传给 Android 链接器，报 `ld.lld: error: aipp-app-manifest.lib: unknown file type`。修法：改为运行时读取 `CARGO_CFG_TARGET_OS == "windows"` 判断目标平台，非 Windows 目标走 `tauri_build::build()`。桌面 Windows 目标路径完全不变。

## 验证

- 桌面 `cargo check --manifest-path src-tauri/Cargo.toml`：exit 0（仅既有 warning）。
- Android 第一次 `pnpm run tauri android build --debug`：172 个编译错误全部消除，链接阶段暴露上述 build.rs 问题（exit 1）。
- Android 第二次构建（build.rs 修复后）：Rust lib 编译+链接全部通过（libaipp_lib.so 产出），但 Gradle 打包阶段 Kotlin daemon 崩溃：`IllegalArgumentException: this and base files have different roots`（项目在 E: 盘、Cargo registry 在 C: 盘，Kotlin 增量编译 Path.relativize 无法跨盘符）。
- 修法：`src-tauri/gen/android/gradle.properties` 追加 `kotlin.incremental=false`（生成目录但已被 git 跟踪）。
- Android 第三次构建：Kotlin daemon 问题解决，Gradle 正常执行；但 tauri android build 默认构建全部 ABI（aarch64/armv7/i686/x86_64），`cranelift-codegen`（wasmtime 依赖链）build script 不支持 armv7（"no supported isa found for arch `armv7`"）。约束禁止改 Cargo.toml 依赖，且任务目标本就是 aarch64，改用 `pnpm run tauri android build --debug --target aarch64 --apk` 只构建 aarch64。
- Android 第四次构建（`--target aarch64 --apk`）：**exit 0**，产出 `src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk`（约 780MB debug 包）。
- build.rs 改动后的桌面 check 回归：`cargo check --manifest-path src-tauri/Cargo.toml` exit 0，102 个 warning（与改动前数量一致，均为既有 warning）。

## 遗留

- 85 个既有 warning 未处理（按约束不动）。
- 全量 ABI（armv7/i686/x86_64）Android 构建仍不可行：wasmtime/cranelift-codegen 不支持 armv7-android。若未来需要多 ABI，需要把 wasmtime 相关依赖做移动端门控（本次按约束未动 Cargo.toml）。
- 移动端运行期行为未真机验证（仅编译/打包通过）。`search_web`/`fetch_url` 在移动端会返回 `MOBILE_UNSUPPORTED: 浏览器搜索/抓取 不支持移动平台`；Copilot LSP 命令在移动端未注册。
