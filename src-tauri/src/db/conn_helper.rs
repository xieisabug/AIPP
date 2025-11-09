use sea_orm::{DatabaseConnection, DbErr};
use std::sync::Arc;
use tauri::Manager;

/// 从 app_handle 获取全局数据库连接
/// 这是所有数据库操作的统一入口
pub fn get_db_conn(app_handle: &tauri::AppHandle) -> Result<Arc<DatabaseConnection>, DbErr> {
    let state = app_handle
        .try_state::<crate::DatabaseState>()
        .ok_or_else(|| DbErr::Custom("DatabaseState not initialized".to_string()))?;
    
    Ok(state.conn.clone())
}

/// 在正确的 runtime 上下文中执行异步数据库操作
pub fn with_runtime<F, Fut, T>(f: F) -> Result<T, DbErr>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<T, DbErr>>,
{
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(f())),
        Err(_) => {
            let rt = tokio::runtime::Runtime::new()
                .map_err(|e| DbErr::Custom(format!("Failed to create Tokio runtime: {}", e)))?;
            rt.block_on(f())
        }
    }
}
