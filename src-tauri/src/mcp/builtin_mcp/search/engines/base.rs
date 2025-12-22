use htmd::{Element, HtmlToMarkdown, element_handler::Handlers};

/// 搜索引擎通用基础功能
pub struct SearchEngineBase;

impl SearchEngineBase {
    /// 将HTML转换为Markdown格式
    pub fn html_to_markdown(html: &str) -> String {
        // 基本的HTML到Markdown转换
        let mut markdown = html.to_string();
        
        // 清理HTML，只保留主要内容相关的部分
        markdown = Self::extract_main_content(&markdown);
        
        // HTML标签转换为Markdown语法
        markdown = Self::convert_html_tags_to_markdown(&markdown);
        
        // 清理多余的空白行
        let lines: Vec<&str> = markdown.lines().collect();
        let mut cleaned_lines = Vec::new();
        let mut prev_empty = false;
        
        for line in lines {
            let line = line.trim();
            if line.is_empty() {
                if !prev_empty {
                    cleaned_lines.push(String::new());
                    prev_empty = true;
                }
            } else {
                cleaned_lines.push(line.to_string());
                prev_empty = false;
            }
        }
        
        cleaned_lines.join("\n").trim().to_string()
    }

    /// 提取HTML中的主要内容
    fn extract_main_content(html: &str) -> String {
        let mut content = html.to_string();
        
        // 移除脚本和样式标签
        let script_pattern = regex::Regex::new(r"(?is)<script[^>]*>.*?</script>").unwrap();
        content = script_pattern.replace_all(&content, "").to_string();
        
        let style_pattern = regex::Regex::new(r"(?is)<style[^>]*>.*?</style>").unwrap();
        content = style_pattern.replace_all(&content, "").to_string();
        
        // 移除注释
        let comment_pattern = regex::Regex::new(r"<!--.*?-->").unwrap();
        content = comment_pattern.replace_all(&content, "").to_string();
        
        // 尝试提取主要内容区域
        let main_patterns = [
            r"(?is)<main[^>]*>(.*?)</main>",
            r"(?is)<article[^>]*>(.*?)</article>",
            r#"(?is)<div[^>]*id=\"?content\"?[^>]*>(.*?)</div>"#,
            r#"(?is)<div[^>]*class=\"[^"]*content[^"]*\"[^>]*>(.*?)</div>"#,
        ];
        
        for pattern in &main_patterns {
            if let Ok(re) = regex::Regex::new(pattern) {
                if let Some(cap) = re.captures(&content) {
                    if let Some(matched) = cap.get(1) {
                        content = matched.as_str().to_string();
                        break;
                    }
                }
            }
        }
        
        content
    }

    /// 将HTML标签转换为Markdown语法
    fn convert_html_tags_to_markdown(html: &str) -> String {
        let converter = HtmlToMarkdown::builder()
            .skip_tags(vec!["script", "style"])
            .add_handler(vec!["svg"], |_handlers: &dyn Handlers, _: Element| {
                Some("[Svg Image]".into())
            })
            .add_handler(vec!["del"], |handlers: &dyn Handlers, element: Element| {
                let content = handlers.walk_children(&element.node).content;
                Some(format!("~~{}~~", content).into())
            })
            .build();

        match converter.convert(html) {
            Ok(result) => {
                result
            }
            Err(_) => {
                // 如果转换失败，保留原始HTML
                html.to_string()
            }
        }
    }
}
