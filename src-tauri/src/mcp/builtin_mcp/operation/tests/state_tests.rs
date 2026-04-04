/// 操作状态管理测试
///
/// 测试 OperationState 的状态管理功能：
/// - 文件读取记录
/// - Bash 进程状态管理
/// - 权限请求处理
use super::super::state::OperationState;
use super::super::types::{PermissionDecision, PermissionRequestEvent};

/// 测试 OperationState 默认创建
#[tokio::test]
async fn test_operation_state_new() {
    let state = OperationState::new();

    // 验证初始状态为空
    assert!(!state.has_file_been_read("/some/path").await);
    assert!(!state.has_file_been_written("/some/path").await);
    assert!(!state.has_file_been_read_for_conversation("/some/path", Some(1)).await);
    assert!(!state.has_file_been_written_for_conversation("/some/path", Some(1)).await);
    assert!(!state.bash_process_exists("some-id").await);
}

/// 测试文件读取记录功能
///
/// 验证内容：
/// - 记录文件读取后可正确查询
/// - 未读取的文件返回 false
/// - 可清除单个文件记录
/// - 可清除所有记录
#[tokio::test]
async fn test_file_read_record() {
    let state = OperationState::new();
    let path1 = "/tmp/test_file_1.txt";
    let path2 = "/tmp/test_file_2.txt";
    let conversation_a = Some(1);
    let conversation_b = Some(2);

    // 初始状态：文件未被读取
    assert!(!state.has_file_been_read_for_conversation(path1, conversation_a).await);
    assert!(!state.has_file_been_read_for_conversation(path2, conversation_a).await);
    assert!(!state.has_file_been_read_for_conversation(path1, conversation_b).await);

    // 记录文件1已读取
    state.record_file_read_for_conversation(path1, conversation_a).await;
    assert!(state.has_file_been_read_for_conversation(path1, conversation_a).await);
    assert!(!state.has_file_been_read_for_conversation(path2, conversation_a).await);
    assert!(!state.has_file_been_read_for_conversation(path1, conversation_b).await);

    // 记录文件2已读取
    state.record_file_read_for_conversation(path2, conversation_a).await;
    assert!(state.has_file_been_read_for_conversation(path1, conversation_a).await);
    assert!(state.has_file_been_read_for_conversation(path2, conversation_a).await);
    assert!(!state.has_file_been_read_for_conversation(path2, conversation_b).await);

    // 清除文件1记录
    state.clear_file_read_for_conversation(path1, conversation_a).await;
    assert!(!state.has_file_been_read_for_conversation(path1, conversation_a).await);
    assert!(state.has_file_been_read_for_conversation(path2, conversation_a).await);

    // 清除所有记录
    state.clear_all_file_reads().await;
    assert!(!state.has_file_been_read_for_conversation(path1, conversation_a).await);
    assert!(!state.has_file_been_read_for_conversation(path2, conversation_a).await);
}

/// 测试文件写入记录功能
#[tokio::test]
async fn test_file_write_record() {
    let state = OperationState::new();
    let path = "/tmp/test_file_write.txt";
    let conversation_a = Some(1);
    let conversation_b = Some(2);

    assert!(!state.has_file_been_written_for_conversation(path, conversation_a).await);

    state.record_file_write_for_conversation(path, conversation_a).await;
    assert!(state.has_file_been_written_for_conversation(path, conversation_a).await);
    assert!(!state.has_file_been_written_for_conversation(path, conversation_b).await);

    state.clear_file_write_for_conversation(path, conversation_a).await;
    assert!(!state.has_file_been_written_for_conversation(path, conversation_a).await);

    state.record_file_write_for_conversation(path, conversation_a).await;
    state.clear_all_file_writes().await;
    assert!(!state.has_file_been_written_for_conversation(path, conversation_a).await);
}

/// 测试 Bash 进程存储和输出追加
///
/// 验证内容：
/// - 存储进程后可查询存在
/// - 可追加输出内容
/// - 可获取增量输出
/// - 可标记进程完成
#[tokio::test]
async fn test_bash_process_management() {
    let state = OperationState::new();
    let bash_id = "test-bash-123";

    // 初始状态：进程不存在
    assert!(!state.bash_process_exists(bash_id).await);

    // 模拟存储进程（使用 echo 命令创建一个简单的 Child）
    // 注意：这里我们直接测试状态管理，不实际创建子进程
    // 因为 store_bash_process 需要真实的 Child，我们测试其他方法

    // 测试输出追加（通过直接操作 bash_processes）
    {
        let mut processes = state.bash_processes.lock().await;
        processes.insert(
            bash_id.to_string(),
            super::super::state::BashProcessInfo {
                child: None,
                output_buffer: String::new(),
                completed: false,
                exit_code: None,
                last_read_pos: 0,
            },
        );
    }

    // 现在进程应该存在
    assert!(state.bash_process_exists(bash_id).await);

    // 追加输出
    state.append_bash_output(bash_id, "line 1\n").await;
    state.append_bash_output(bash_id, "line 2\n").await;

    // 获取增量输出
    let (output, completed, exit_code) = state.get_bash_incremental_output(bash_id).await.unwrap();
    assert_eq!(output, "line 1\nline 2\n");
    assert!(!completed);
    assert_eq!(exit_code, None);

    // 再次获取应该返回空（因为已经读取过了）
    let (output, _, _) = state.get_bash_incremental_output(bash_id).await.unwrap();
    assert_eq!(output, "");

    // 追加更多输出
    state.append_bash_output(bash_id, "line 3\n").await;

    // 应该只返回新增的内容
    let (output, _, _) = state.get_bash_incremental_output(bash_id).await.unwrap();
    assert_eq!(output, "line 3\n");

    // 标记完成
    state.mark_bash_completed(bash_id, Some(0)).await;

    let (_, completed, exit_code) = state.get_bash_incremental_output(bash_id).await.unwrap();
    assert!(completed);
    assert_eq!(exit_code, Some(0));

    // 移除进程
    state.remove_bash_process(bash_id).await;
    assert!(!state.bash_process_exists(bash_id).await);
}

/// 测试权限请求处理
///
/// 验证内容：
/// - 存储权限请求后可通过 resolve 发送响应
/// - 响应后请求被移除
#[tokio::test]
async fn test_permission_request_handling() {
    let state = OperationState::new();
    let request_id = "perm-request-123";

    // 创建 oneshot 通道
    let (tx, rx) = tokio::sync::oneshot::channel();

    // 存储权限请求
    state
        .store_permission_request(
            PermissionRequestEvent {
                request_id: request_id.to_string(),
                operation: "read_file".to_string(),
                path: "/tmp/example.txt".to_string(),
                conversation_id: Some(42),
            },
            tx,
        )
        .await;
    assert!(state.has_pending_permission_for_conversation(42).await);
    let snapshot = state.get_permission_request(request_id).await.expect("snapshot should exist");
    assert_eq!(snapshot.review_code, "OP-PERMRE");

    // 在另一个任务中解决权限请求
    let state_clone = state.clone();
    let request_id_clone = request_id.to_string();
    tokio::spawn(async move {
        // 模拟用户响应
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let resolution = state_clone
            .resolve_permission_request(&request_id_clone, PermissionDecision::Allow)
            .await;
        assert!(resolution.as_ref().is_some_and(|value| value.delivered));
        assert_eq!(resolution.and_then(|value| value.conversation_id), Some(42));
    });

    // 等待接收响应
    let decision = rx.await.unwrap();
    assert_eq!(decision, PermissionDecision::Allow);

    // 再次尝试解决应该失败（已被移除）
    assert!(!state.has_pending_permission_for_conversation(42).await);
    let resolution = state.resolve_permission_request(request_id, PermissionDecision::Deny).await;
    assert!(resolution.is_none());
}

/// 测试 OperationState Clone
#[tokio::test]
async fn test_operation_state_clone() {
    let state1 = OperationState::new();
    let path = "/tmp/shared_file.txt";
    let conversation_id = Some(7);

    // 在 state1 中记录文件读取
    state1.record_file_read_for_conversation(path, conversation_id).await;

    // Clone state
    let state2 = state1.clone();

    // state2 应该能看到 state1 的记录（共享 Arc）
    assert!(state2.has_file_been_read_for_conversation(path, conversation_id).await);

    // 在 state2 中清除记录
    state2.clear_file_read_for_conversation(path, conversation_id).await;

    // state1 也应该看到变化
    assert!(!state1.has_file_been_read_for_conversation(path, conversation_id).await);
}

/// 测试文件读写记录按 conversation 隔离
#[tokio::test]
async fn test_file_records_are_conversation_scoped() {
    let state = OperationState::new();
    let path = "/tmp/conversation_scoped.txt";
    let conversation_a = Some(100);
    let conversation_b = Some(200);

    state.record_file_read_for_conversation(path, conversation_a).await;
    state.record_file_write_for_conversation(path, conversation_a).await;

    assert!(state.has_file_been_read_for_conversation(path, conversation_a).await);
    assert!(state.has_file_been_written_for_conversation(path, conversation_a).await);
    assert!(!state.has_file_been_read_for_conversation(path, conversation_b).await);
    assert!(!state.has_file_been_written_for_conversation(path, conversation_b).await);
    assert!(!state.has_file_been_read(path).await);
    assert!(!state.has_file_been_written(path).await);
}
