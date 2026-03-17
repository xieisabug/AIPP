use chrono::Local;
use futures::future::BoxFuture;
use futures::FutureExt;
use htmd;
use regex::Regex;
use reqwest;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::debug;

// 用于 HTML 内容清理
use crate::mcp::builtin_mcp::search::engines::base::SearchEngineBase;
mod plugin_bangs;
pub use plugin_bangs::build_template_engine;

// 定义命令处理函数类型
pub type CommandFn = Arc<
    dyn Fn(TemplateEngine, String, HashMap<String, String>) -> BoxFuture<'static, String>
        + Send
        + Sync,
>;

fn wrap_command(
    handler: fn(TemplateEngine, String, HashMap<String, String>) -> BoxFuture<'static, String>,
) -> CommandFn {
    Arc::new(move |engine, input, context| handler(engine, input, context))
}

// 获取当前日期的命令处理函数
fn current_date(
    _: TemplateEngine,
    _: String,
    _: HashMap<String, String>,
) -> BoxFuture<'static, String> {
    async { Local::now().format("%Y-%m-%d").to_string() }.boxed()
}

// 获取当前时间的命令处理函数
fn current_time(
    _: TemplateEngine,
    _: String,
    _: HashMap<String, String>,
) -> BoxFuture<'static, String> {
    async { Local::now().format("%H:%M:%S").to_string() }.boxed()
}

// 截取指定长度字符的命令处理函数
fn sub_start(
    engine: TemplateEngine,
    input: String,
    context: HashMap<String, String>,
) -> BoxFuture<'static, String> {
    async move {
        debug!(input = %input, "sub_start input");
        let re = Regex::new(r"\((.*),(\d+)\)").unwrap();
        for cap in re.captures_iter(&input) {
            debug!(?cap, "sub_start capture");
            let text_origin = &cap[1];
            let num = &cap[2];

            let text = engine.parse(text_origin.trim(), &context).await;
            if let Ok(count) = num.trim().parse::<usize>() {
                return text.chars().take(count).collect();
            }
        }
        String::new()
    }
    .boxed()
}

fn selected_text(
    _: TemplateEngine,
    _: String,
    context: HashMap<String, String>,
) -> BoxFuture<'static, String> {
    async move { context.get("selected_text").unwrap_or(&String::default()).to_string() }.boxed()
}

// 新增获取网页内容的函数
fn web(_: TemplateEngine, url: String, _: HashMap<String, String>) -> BoxFuture<'static, String> {
    async move {
        // 移除url中前后的括号
        let url = url.trim_start_matches('(').trim_end_matches(')');

        let client = reqwest::Client::builder().danger_accept_invalid_certs(true).build().unwrap();

        match client.get(url).send().await {
            Ok(response) => {
                let html = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "Failed to get web content".to_string());
                // 清理HTML：只保留body，移除script/style/nav等非核心内容
                let cleaned_html = SearchEngineBase::extract_main_content(&html);
                format!("<bangweb url=\"{}\">\n{}\n</bangweb>", url, cleaned_html)
            }
            Err(err) => err.to_string(),
        }
    }
    .boxed()
}

// 新增获取网页内容并转换为 Markdown 的函数
fn web_to_markdown(
    _: TemplateEngine,
    url: String,
    _: HashMap<String, String>,
) -> BoxFuture<'static, String> {
    async move {
        // 移除url中前后的括号
        let url = url.trim_start_matches('(').trim_end_matches(')');

        let client = reqwest::Client::new();
        match client.get(url).send().await {
            Ok(response) => {
                let html = response.text().await.unwrap_or_default();
                // 先清理HTML，再转换为Markdown
                let cleaned_html = SearchEngineBase::extract_main_content(&html);
                format!(
                    "<bangwebtomarkdown url=\"{}\">\n{}\n</bangwebtomarkdown>",
                    url,
                    htmd::convert(&cleaned_html).unwrap()
                )
            }
            Err(_) => "".to_string(),
        }
    }
    .boxed()
}

// 读取文本文件内容的函数
fn file_command(
    _: TemplateEngine,
    path: String,
    _: HashMap<String, String>,
) -> BoxFuture<'static, String> {
    async move {
        // 移除路径中前后的括号和空格
        let path = path.trim_start_matches('(').trim_end_matches(')').trim();

        // 支持带引号的路径（处理路径中可能的空格）
        let path = path
            .trim_start_matches('"')
            .trim_end_matches('"')
            .trim_start_matches('\'')
            .trim_end_matches('\'');

        match std::fs::read_to_string(path) {
            Ok(content) => {
                format!("<bangfile path=\"{}\">\n{}\n</bangfile>", path, content)
            }
            Err(err) => {
                format!("<!file_error: 无法读取文件 '{}' - {}>", path, err)
            }
        }
    }
    .boxed()
}

// 模板解析器结构体
#[derive(Clone)]
pub struct TemplateEngine {
    commands: HashMap<String, Bang>,
}

#[derive(Clone)]
pub struct Bang {
    pub name: String,
    pub complete: String,
    pub description: String,
    pub bang_type: BangType,
    pub command: CommandFn,
}

#[derive(Clone, Serialize)]
pub enum BangType {
    Text,
    Image,
    Audio,
}

impl Bang {
    pub fn new(
        name: impl Into<String>,
        complete: impl Into<String>,
        description: impl Into<String>,
        bang_type: BangType,
        command: CommandFn,
    ) -> Self {
        Self {
            name: name.into(),
            complete: complete.into(),
            description: description.into(),
            bang_type,
            command,
        }
    }
}

impl TemplateEngine {
    // 初始化模板解析器
    pub fn new() -> Self {
        let mut commands = HashMap::new();

        commands.insert(
            "current_date".to_string(),
            Bang::new(
                "current_date",
                "current_date",
                "获取当前日期",
                BangType::Text,
                wrap_command(current_date),
            ),
        );
        commands.insert(
            "cd".to_string(),
            Bang::new("cd", "cd", "获取当前日期", BangType::Text, wrap_command(current_date)),
        );

        commands.insert(
            "current_time".to_string(),
            Bang::new(
                "current_time",
                "current_time",
                "获取当前时间",
                BangType::Text,
                wrap_command(current_time),
            ),
        );
        commands.insert(
            "ct".to_string(),
            Bang::new("ct", "ct", "获取当前时间", BangType::Text, wrap_command(current_time)),
        );

        commands.insert(
            "sub_start".to_string(),
            Bang::new(
                "sub_start",
                "sub_start(|)",
                "截取文本的前多少个字符",
                BangType::Text,
                wrap_command(sub_start),
            ),
        );

        commands.insert(
            "selected_text".to_string(),
            Bang::new(
                "selected_text",
                "selected_text",
                "获取当前选中的文本",
                BangType::Text,
                wrap_command(selected_text),
            ),
        );
        commands.insert(
            "s".to_string(),
            Bang::new("s", "s", "获取当前选中的文本", BangType::Text, wrap_command(selected_text)),
        );

        commands.insert(
            "web".to_string(),
            Bang::new(
                "web",
                "web(|)",
                "通过网络获取URL的网页信息",
                BangType::Text,
                wrap_command(web),
            ),
        );
        commands.insert(
            "w".to_string(),
            Bang::new("w", "w(|)", "通过网络获取URL的网页信息", BangType::Text, wrap_command(web)),
        );

        commands.insert(
            "web_to_markdown".to_string(),
            Bang::new(
                "web_to_markdown",
                "web_to_markdown(|)",
                "通过网络获取URL的网页信息并且转换为markdown格式",
                BangType::Text,
                wrap_command(web_to_markdown),
            ),
        );
        commands.insert(
            "wm".to_string(),
            Bang::new(
                "wm",
                "wm(|)",
                "通过网络获取URL的网页信息并且转换为markdown格式",
                BangType::Text,
                wrap_command(web_to_markdown),
            ),
        );

        commands.insert(
            "file".to_string(),
            Bang::new(
                "file",
                "file(|)",
                "读取文本文件内容",
                BangType::Text,
                wrap_command(file_command),
            ),
        );

        TemplateEngine { commands }
    }

    // 注册命令
    pub fn register_command(&mut self, name: &str, handler: CommandFn) {
        self.register_bang(Bang {
            name: name.to_string(),
            complete: name.to_string(),
            description: "Custom command".to_string(),
            bang_type: BangType::Text,
            command: handler,
        });
    }

    pub fn register_builtin_command(
        &mut self,
        name: &str,
        handler: fn(TemplateEngine, String, HashMap<String, String>) -> BoxFuture<'static, String>,
    ) {
        self.register_command(name, wrap_command(handler));
    }

    pub fn register_bang(&mut self, bang: Bang) {
        self.commands.insert(bang.name.clone(), bang);
    }

    pub fn has_command(&self, name: &str) -> bool {
        self.commands.contains_key(name)
    }

    // 解析并替换模板字符串
    pub async fn parse(&self, template: &str, context: &HashMap<String, String>) -> String {
        let re = Regex::new(r"[!！](\w+)(\((?:[^()]|\((?:[^()]|\((?:[^()]|\((?:[^()]|\((?:[^()]|\((?:[^()]|\((?:[^()]|\((?:[^()]|\((?:[^()]|\([^()]*\))*\))*\))*\))*\))*\))*\))*\))*\))*\))?").unwrap();
        let mut result = template.to_string();

        for cap in re.captures_iter(template) {
            debug!(?cap, "parse bang capture");
            let command = &cap[1];
            let args = cap.get(2).map_or("", |m| m.as_str());
            if let Some(bang) = self.commands.get(command) {
                let replacement =
                    (bang.command)(self.clone(), args.to_string(), context.clone()).await;
                result = result.replace(&cap[0], &replacement);
            }
        }

        // 替换上下文变量
        for (key, value) in context {
            let placeholder = format!("!{}", key);
            result = result.replace(&placeholder, value);
        }

        result
    }

    pub fn get_commands(&self) -> Vec<Bang> {
        self.commands.values().cloned().collect()
    }
}

#[cfg(test)]
mod tests;
