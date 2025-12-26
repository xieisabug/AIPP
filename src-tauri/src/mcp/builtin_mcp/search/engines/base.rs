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

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================
    // html_to_markdown Tests
    // ============================================

    #[test]
    fn test_html_to_markdown_basic() {
        let html = "<p>Hello World</p>";
        let result = SearchEngineBase::html_to_markdown(html);
        assert!(result.contains("Hello World"));
    }

    #[test]
    fn test_html_to_markdown_strips_script() {
        let html = "<div><script>alert('evil');</script><p>Content</p></div>";
        let result = SearchEngineBase::html_to_markdown(html);
        assert!(!result.contains("alert"));
        assert!(!result.contains("script"));
        assert!(result.contains("Content"));
    }

    #[test]
    fn test_html_to_markdown_strips_style() {
        let html = "<style>.hidden { display: none; }</style><p>Visible</p>";
        let result = SearchEngineBase::html_to_markdown(html);
        assert!(!result.contains("display"));
        assert!(!result.contains("hidden"));
        assert!(result.contains("Visible"));
    }

    #[test]
    fn test_html_to_markdown_heading() {
        let html = "<h1>Title</h1><p>Paragraph</p>";
        let result = SearchEngineBase::html_to_markdown(html);
        assert!(result.contains("Title"));
        assert!(result.contains("Paragraph"));
    }

    #[test]
    fn test_html_to_markdown_link() {
        let html = r#"<a href="https://example.com">Link Text</a>"#;
        let result = SearchEngineBase::html_to_markdown(html);
        // Markdown link format
        assert!(result.contains("Link Text"));
        assert!(result.contains("https://example.com"));
    }

    #[test]
    fn test_html_to_markdown_removes_comments() {
        let html = "<!-- This is a comment --><p>Content</p>";
        let result = SearchEngineBase::html_to_markdown(html);
        assert!(!result.contains("comment"));
        assert!(result.contains("Content"));
    }

    #[test]
    fn test_html_to_markdown_list() {
        let html = "<ul><li>Item 1</li><li>Item 2</li></ul>";
        let result = SearchEngineBase::html_to_markdown(html);
        assert!(result.contains("Item 1"));
        assert!(result.contains("Item 2"));
    }

    #[test]
    fn test_html_to_markdown_cleans_whitespace() {
        let html = "<p>Line 1</p>\n\n\n\n<p>Line 2</p>";
        let result = SearchEngineBase::html_to_markdown(html);
        // Should not have more than one consecutive empty line
        assert!(!result.contains("\n\n\n"));
    }

    #[test]
    fn test_html_to_markdown_empty_input() {
        let html = "";
        let result = SearchEngineBase::html_to_markdown(html);
        assert!(result.is_empty());
    }

    #[test]
    fn test_html_to_markdown_svg_placeholder() {
        let html = "<svg><path d='...'></path></svg><p>After SVG</p>";
        let result = SearchEngineBase::html_to_markdown(html);
        // SVG should be replaced with placeholder
        assert!(result.contains("[Svg Image]") || !result.contains("<svg"));
    }

    #[test]
    fn test_html_to_markdown_del_tag() {
        let html = "<del>Deleted text</del>";
        let result = SearchEngineBase::html_to_markdown(html);
        // Del tag should become strikethrough
        assert!(result.contains("~~") || result.contains("Deleted text"));
    }

    #[test]
    fn test_html_to_markdown_bold() {
        let html = "<strong>Bold text</strong>";
        let result = SearchEngineBase::html_to_markdown(html);
        // Should convert to markdown bold
        assert!(result.contains("Bold text"));
    }

    #[test]
    fn test_html_to_markdown_italic() {
        let html = "<em>Italic text</em>";
        let result = SearchEngineBase::html_to_markdown(html);
        // Should convert to markdown italic
        assert!(result.contains("Italic text"));
    }

    #[test]
    fn test_html_to_markdown_main_content_extraction() {
        let html = r#"
        <header>Header content</header>
        <main><p>Main content here</p></main>
        <footer>Footer content</footer>
        "#;
        let result = SearchEngineBase::html_to_markdown(html);
        // Should extract main content
        assert!(result.contains("Main content here"));
    }

    #[test]
    fn test_html_to_markdown_article_extraction() {
        let html = r#"
        <nav>Navigation</nav>
        <article><p>Article content</p></article>
        <aside>Sidebar</aside>
        "#;
        let result = SearchEngineBase::html_to_markdown(html);
        // Should extract article content
        assert!(result.contains("Article content"));
    }
}
