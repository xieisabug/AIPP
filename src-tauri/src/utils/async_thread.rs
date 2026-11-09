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
