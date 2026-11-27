//! 需要用户确认的命令，统一由前端应用内确认框询问（不弹系统原生对话框）。
//!
//! 命令第一次调用不带 `confirmed_prompt`，后端算出确认文案后返回 `Confirm { prompt }`；
//! 前端显示该文案，用户同意后把同一段文案原样传回再调用一次。后端重新生成文案并与
//! 传回的比较：一致才执行；期间内容变化（文案不同）则再次返回新的文案，
//! 保证用户看到的就是这次实际执行的内容。

use serde::Serialize;

#[derive(Debug, Serialize, PartialEq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Consent<T> {
    Done { value: T },
    Confirm { prompt: String },
}

/// 用户是否已经确认了这段文案。
pub fn approved(prompt: &str, confirmed_prompt: Option<&str>) -> bool {
    confirmed_prompt == Some(prompt)
}
