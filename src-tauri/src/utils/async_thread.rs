use std::future::Future;
use std::io;
use std::thread::JoinHandle;

/// 在独立线程上 `block_on` 运行的 async 任务使用的栈大小。
///
/// `block_on` 在当前线程的栈上 poll 整个 future。工具执行、续写、管家派发这些链路
/// 嵌套很深（例如 execute_mcp_tool_call → search_web → chromiumoxide 启动浏览器），
/// debug 构建下 future 体积很大，`std::thread::spawn` 默认的 2MB 栈会溢出，
/// 整个进程以 STATUS_STACK_OVERFLOW 退出。栈是按需提交的虚拟内存，放大不会立即占用物理内存。
pub const ASYNC_THREAD_STACK_SIZE: usize = 32 * 1024 * 1024;

/// 在有名字、栈足够大的独立线程上运行 `make_future()` 产生的 future。
///
/// 用于必须脱离调用方 async 上下文的场景：future 不满足 `Send`，或需要切断
/// 工具执行与 ask_ai 之间的异步递归。传入闭包而不是 future，future 在新线程内创建，
/// 因此本身不需要 `Send`。线程名会出现在 panic/栈溢出信息里，便于定位来源。
pub fn spawn_async_thread<F, Fut, T>(name: &str, make_future: F) -> io::Result<JoinHandle<T>>
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = T> + 'static,
    T: Send + 'static,
{
    spawn_large_stack_thread(name, move || tauri::async_runtime::block_on(make_future()))
}

/// 与 [`spawn_async_thread`] 相同的线程配置，供闭包内自行 `block_on` 并处理结果的调用方使用。
pub fn spawn_large_stack_thread<F, T>(name: &str, f: F) -> io::Result<JoinHandle<T>>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    std::thread::Builder::new()
        .name(name.to_string())
        .stack_size(ASYNC_THREAD_STACK_SIZE)
        .spawn(f)
}

/// 在大栈线程上运行 `make_future()`，当前任务异步等待结果（不阻塞当前线程）。
///
/// 供运行在 tokio 工作线程上的入口使用：Tauri async 命令、`tauri::async_runtime::spawn`
/// 的任务都跑在默认 2MB 栈的工作线程上，直接 poll 工具执行链会 STATUS_STACK_OVERFLOW。
/// 当前 tracing span 会带到新线程，日志仍挂在调用方的 span 下。
pub async fn run_on_large_stack<F, Fut, T>(name: &str, make_future: F) -> Result<T, String>
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = T> + 'static,
    T: Send + 'static,
{
    use tracing::Instrument;

    let span = tracing::Span::current();
    let (sender, receiver) = tokio::sync::oneshot::channel();
    spawn_async_thread(name, move || {
        async move {
            let _ = sender.send(make_future().await);
        }
        .instrument(span)
    })
    .map_err(|error| format!("无法启动执行线程 {name}：{error}"))?;
    receiver
        .await
        .map_err(|_| format!("执行线程 {name} 没有返回结果就结束了（线程内发生了 panic）"))
}
