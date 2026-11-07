use super::async_thread::spawn_async_thread;

#[test]
fn test_spawn_async_thread_runs_future_on_named_thread() {
    let handle = spawn_async_thread("aipp-test-async", || async {
        std::thread::current().name().map(str::to_string)
    })
    .expect("spawn thread");
    assert_eq!(handle.join().unwrap().as_deref(), Some("aipp-test-async"));
}

#[test]
fn test_spawn_async_thread_accepts_non_send_future() {
    let handle = spawn_async_thread("aipp-test-non-send", || async {
        let local = std::rc::Rc::new(41);
        tokio::task::yield_now().await;
        *local + 1
    })
    .expect("spawn thread");
    assert_eq!(handle.join().unwrap(), 42);
}

#[test]
fn test_spawn_async_thread_survives_deep_stack_usage() {
    // 远超 std::thread 默认 2MB 栈的局部数据，模拟 debug 构建下体积很大的 future。
    let handle = spawn_async_thread("aipp-test-deep", || async {
        let buffer = std::hint::black_box([7u8; 8 * 1024 * 1024]);
        buffer.iter().map(|&b| b as u64).sum::<u64>()
    })
    .expect("spawn thread");
    assert_eq!(handle.join().unwrap(), 7 * 8 * 1024 * 1024);
}
