use tauri::AppHandle;
use tracing::{debug, info, instrument};

use super::bash_ops::BashOperations;
use super::file_ops::FileOperations;
use super::permission::PermissionManager;
use super::state::OperationState;
use super::types::*;

/// 操作工具处理器
#[derive(Clone)]
pub struct OperationHandler {
    app_handle: AppHandle,
}

impl OperationHandler {
    pub fn new(app_handle: AppHandle) -> Self {
        debug!("Creating OperationHandler");
        Self { app_handle }
    }

    /// 获取权限管理器
    fn permission_manager(&self) -> PermissionManager {
        PermissionManager::new(self.app_handle.clone())
    }

    /// 读取文件
    #[instrument(skip(self, state), fields(file_path = %request.file_path))]
    pub async fn read_file(
        &self,
        state: &OperationState,
        request: ReadFileRequest,
        conversation_id: Option<i64>,
    ) -> Result<ReadFileResponse, String> {
        info!("Handling read_file request");
        FileOperations::read_file(state, &self.permission_manager(), request, conversation_id).await
    }

    /// 写入文件
    #[instrument(skip(self, state, request), fields(file_path = %request.file_path))]
    pub async fn write_file(
        &self,
        state: &OperationState,
        request: WriteFileRequest,
        conversation_id: Option<i64>,
    ) -> Result<WriteFileResponse, String> {
        info!("Handling write_file request");
        FileOperations::write_file(state, &self.permission_manager(), request, conversation_id).await
    }

    /// 编辑文件
    #[instrument(skip(self, state, request), fields(file_path = %request.file_path))]
    pub async fn edit_file(
        &self,
        state: &OperationState,
        request: EditFileRequest,
        conversation_id: Option<i64>,
    ) -> Result<EditFileResponse, String> {
        info!("Handling edit_file request");
        FileOperations::edit_file(state, &self.permission_manager(), request, conversation_id).await
    }

    /// 列出目录
    #[instrument(skip(self, state), fields(path = %request.path))]
    pub async fn list_directory(
        &self,
        state: &OperationState,
        request: ListDirectoryRequest,
        conversation_id: Option<i64>,
    ) -> Result<ListDirectoryResponse, String> {
        info!("Handling list_directory request");
        FileOperations::list_directory(state, &self.permission_manager(), request, conversation_id).await
    }

    /// 执行 Bash 命令
    #[instrument(skip(self, state), fields(command = %request.command))]
    pub async fn execute_bash(
        &self,
        state: &OperationState,
        request: ExecuteBashRequest,
    ) -> Result<ExecuteBashResponse, String> {
        info!("Handling execute_bash request");
        BashOperations::execute_bash(state, request).await
    }

    /// 获取 Bash 输出
    #[instrument(skip(self, state), fields(bash_id = %request.bash_id))]
    pub async fn get_bash_output(
        &self,
        state: &OperationState,
        request: GetBashOutputRequest,
    ) -> Result<GetBashOutputResponse, String> {
        info!("Handling get_bash_output request");
        BashOperations::get_bash_output(state, request).await
    }
}
