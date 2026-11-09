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

/// 找出在阻塞线程池（默认 2MB 栈）里 `block_on` 的写法。工具执行、续写和群聊回复都会再进入 ask_ai，
/// debug 构建下 future 很深，这样写会让整个进程以 STATUS_STACK_OVERFLOW 退出；应改用 `spawn_async_thread`。
fn blocking_pool_block_on_sites(source: &str) -> Vec<usize> {
    let lines = source.lines().collect::<Vec<_>>();
    lines.iter().enumerate()
        .filter(|(_, line)| line.contains("spawn_blocking(") && !line.trim_start().starts_with("//"))
        .filter(|(index, _)| lines[*index..lines.len().min(index + 4)].iter().any(|next| next.contains("block_on(")))
        .map(|(index, _)| index + 1)
        .collect()
}

#[test]
fn test_blocking_pool_block_on_detector_flags_the_overflowing_pattern() {
    let bad = "tauri::async_runtime::spawn_blocking(move || {\n    tauri::async_runtime::block_on(async move { run().await });\n});";
    assert_eq!(blocking_pool_block_on_sites(bad), vec![1]);
    let good = "tauri::async_runtime::spawn_blocking(move || read_db());\nspawn_async_thread(\"x\", move || async move { run().await });";
    assert!(blocking_pool_block_on_sites(good).is_empty());
}

#[test]
fn test_no_backend_path_block_on_inside_blocking_pool() {
    fn visit(dir: &std::path::Path, found: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).expect("read source dir") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                visit(&path, found);
            } else if path.extension().is_some_and(|ext| ext == "rs") && !path.ends_with("async_thread_tests.rs") {
                let source = std::fs::read_to_string(&path).expect("read source file");
                for line in blocking_pool_block_on_sites(&source) {
                    found.push(format!("{}:{line}", path.display()));
                }
            }
        }
    }
    let mut found = Vec::new();
    visit(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut found);
    assert!(found.is_empty(), "这些位置在 2MB 栈的阻塞线程池里 block_on，改用 spawn_async_thread：\n{}", found.join("\n"));
}
