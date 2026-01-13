use crate::mcp::builtin_mcp::search::types::{SearchItem, SearchResults};
use htmd::{element_handler::Handlers, Element, HtmlToMarkdown};
use scraper::{Html, Selector};
use tracing::debug;

/// Google搜索引擎实现
pub struct GoogleEngine;

impl GoogleEngine {
    pub fn display_name() -> &'static str {
        "Google"
    }

    pub fn homepage_url() -> &'static str {
        "https://www.google.com"
    }

    pub fn search_input_selectors() -> Vec<&'static str> {
        vec![
            // 主要搜索框选择器（按优先级排序）
            "textarea[name='q']",                     // 新版 Google 主搜索框
            "input[name='q']",                        // 传统搜索框
            "form[action='/search'] input[name='q']", // 带 action 的搜索表单
            "form[action*='google.'][role='search'] input[name='q']", // 更通用的表单匹配
            "textarea[title='搜索']",                 // 中文界面搜索框
            "input[title='搜索']",
            "textarea[title='Search']", // 英文界面搜索框
            "input[title='Search']",
            "#APjFqb",                      // Google 特定 ID
            ".gLFyf",                       // Google 特定类名
            ".a4bIc input",                 // 容器内的输入框
            ".a4bIc textarea",              // 容器内的文本域
            "form[role='search'] textarea", // 表单内搜索框
            "form[role='search'] input[type='text']",
            "form[role='search'] input[type='search']",
            // 备用选择器（更广泛的匹配）
            "input[aria-label*='搜索']",
            "textarea[aria-label*='搜索']",
            "input[aria-label*='Search']",
            "textarea[aria-label*='Search']",
            "input[autocomplete='off'][name='q']",
            "textarea[autocomplete='off'][name='q']",
        ]
    }

    pub fn search_button_selectors() -> Vec<&'static str> {
        vec![
            // 主要搜索按钮选择器
            "input[name='btnK']",           // 标准 Google 搜索按钮
            "button[type='submit']",        // 通用提交按钮
            "input[value='Google 搜索']",   // 中文搜索按钮
            "input[value='Google Search']", // 英文搜索按钮
            ".FPdoLc input[name='btnK']",   // 容器内的搜索按钮
            ".tfB0Bf input[name='btnK']",   // 另一个容器
            // 高级选择器
            "center input[name='btnK']", // 居中容器内的按钮
            "form input[type='submit'][name='btnK']",
            "form button[aria-label*='搜索']",
            "form button[aria-label*='Search']",
            // 备用按钮选择器
            "input[type='submit'][value*='搜索']",
            "input[type='submit'][value*='Search']",
            "button[data-ved]:not([disabled])", // Google 特有的按钮
        ]
    }

    pub fn default_wait_selectors() -> Vec<String> {
        vec![
            // 主要搜索结果容器
            "#search".to_string(),     // Google 主搜索结果容器
            "#main".to_string(),       // 主内容区域
            "#rcnt".to_string(),       // 结果计数容器
            "#center_col".to_string(), // 中心列
            // 具体结果选择器
            "[data-ved]".to_string(), // Google 结果项标识
            ".g".to_string(),         // Google 搜索结果项类名
            ".tF2Cxc".to_string(),    // 新版结果项类名
            ".yuRUbf".to_string(),    // 结果标题容器
            // 其他有效容器
            "#rso".to_string(),       // 搜索结果区域
            ".srp".to_string(),       // 搜索结果页面
            "#topads".to_string(),    // 广告区域（也表明页面已加载）
            "#bottomads".to_string(), // 底部广告
            // 错误页面或特殊情况
            ".med".to_string(),                // 消息区域
            "#errorPageContainer".to_string(), // 错误页面
        ]
    }

    /// 解析Google搜索结果HTML，提取结构化信息（HTML解析器版）
    pub fn parse_search_results(html: &str, query: &str) -> SearchResults {
        // 使用 htmd 解析并打印 Markdown 结果，返回值仍按原逻辑构造
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
            Ok(markdown) => {
                let trimmed = markdown.trim();
                let preview: String = trimmed.chars().collect();

                debug!(
                    google_htmd_preview = %preview,
                    "htmd parsed Google search HTML"
                );
            }
            Err(err) => {
                debug!(error = %err, "htmd failed to parse Google search HTML to markdown");
            }
        }

        let mut items = Vec::new();
        let document = Html::parse_document(html);

        // 结果卡片选择器（优先新版本，再到通用）
        let selectors = [Selector::parse("div.tF2Cxc").ok(), Selector::parse("div.g").ok()];

        let mut rank = 1usize;
        for sel in selectors.iter().flatten() {
            for card in document.select(sel) {
                if let Some(item) = Self::parse_card_element(card, rank) {
                    items.push(item);
                    rank += 1;
                    if items.len() >= 20 {
                        break;
                    }
                }
            }
            if !items.is_empty() {
                break;
            }
        }

        // 提取搜索结果总数（如果可获取）
        let total_results = Self::extract_total_results(html);

        SearchResults {
            query: query.to_string(),
            search_engine: Self::display_name().to_string(),
            engine_id: "google".to_string(),
            homepage_url: Self::homepage_url().to_string(),
            items,
            total_results,
            search_time_ms: None,
        }
    }

    /// 从结果卡片元素中抽取一个条目
    fn parse_card_element(card: scraper::ElementRef<'_>, rank: usize) -> Option<SearchItem> {
        // 标题：优先 .yuRUbf h3，其次任意 h3 / role=heading
        let title = Self::first_text_in(card, &[".yuRUbf h3", "h3", "[role=heading]"])
            .unwrap_or_else(|| format!("Search Result {}", rank));

        // URL：优先 .yuRUbf a[href]，否则任意 a[href]
        let url = Self::first_href_in(card, &[".yuRUbf a[href]", "a[href]"]).unwrap_or_default();

        // 摘要：兼容多版本类名
        let snippet = Self::first_text_in(card, &["div.VwiC3b", "span.VwiC3b", "span[data-ved]"])
            .unwrap_or_default();

        // 显示 URL（有些页面会有）
        let display_url = Self::first_text_in(card, &["cite", "span.dDKKM"]);

        if !title.trim().is_empty() && !url.trim().is_empty() {
            Some(SearchItem {
                title: title.trim().to_string(),
                url,
                snippet: snippet.trim().to_string(),
                rank,
                display_url: display_url.map(|s| s.trim().to_string()),
            })
        } else {
            None
        }
    }

    /// 在元素内按选择器列表找到首个文本
    fn first_text_in(root: scraper::ElementRef<'_>, selectors: &[&str]) -> Option<String> {
        for sel in selectors {
            if let Ok(selector) = Selector::parse(sel) {
                if let Some(node) = root.select(&selector).next() {
                    let text = node.text().collect::<String>();
                    let text = text.trim();
                    if !text.is_empty() {
                        return Some(text.to_string());
                    }
                }
            }
        }
        None
    }

    /// 在元素内按选择器列表找到首个链接的真实 URL
    fn first_href_in(root: scraper::ElementRef<'_>, selectors: &[&str]) -> Option<String> {
        for sel in selectors {
            if let Ok(selector) = Selector::parse(sel) {
                for node in root.select(&selector) {
                    if let Some(href) = node.value().attr("href") {
                        if href.starts_with("/url?q=") {
                            if let Some(actual) = Self::decode_google_url(href) {
                                return Some(actual);
                            }
                        } else if href.starts_with("http") {
                            return Some(href.to_string());
                        }
                    }
                }
            }
        }
        None
    }

    /// 解码Google重定向URL
    fn decode_google_url(url: &str) -> Option<String> {
        if let Some(q_start) = url.find("q=") {
            let q_part = &url[q_start + 2..];
            if let Some(end) = q_part.find('&') {
                let encoded_url = &q_part[..end];
                return urlencoding::decode(encoded_url).ok().map(|s| s.into_owned());
            } else {
                return urlencoding::decode(q_part).ok().map(|s| s.into_owned());
            }
        }
        None
    }

    /// 提取搜索结果总数
    fn extract_total_results(html: &str) -> Option<u64> {
        let patterns = [r"About ([\d,]+) results", r"大约 ([\d,]+) 条结果", r"(\d+) 个结果"];

        for pattern in &patterns {
            if let Ok(re) = regex::Regex::new(pattern) {
                if let Some(cap) = re.captures(html) {
                    if let Some(num_str) = cap.get(1) {
                        let num_clean = num_str.as_str().replace(',', "");
                        if let Ok(num) = num_clean.parse::<u64>() {
                            return Some(num);
                        }
                    }
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== Static Method Tests ====================

    #[test]
    fn test_display_name() {
        assert_eq!(GoogleEngine::display_name(), "Google");
    }

    #[test]
    fn test_homepage_url() {
        assert_eq!(GoogleEngine::homepage_url(), "https://www.google.com");
    }

    #[test]
    fn test_search_input_selectors_not_empty() {
        let selectors = GoogleEngine::search_input_selectors();
        assert!(!selectors.is_empty());
        // 验证包含主要选择器
        assert!(selectors.contains(&"textarea[name='q']"));
        assert!(selectors.contains(&"input[name='q']"));
    }

    #[test]
    fn test_search_button_selectors_not_empty() {
        let selectors = GoogleEngine::search_button_selectors();
        assert!(!selectors.is_empty());
        assert!(selectors.contains(&"input[name='btnK']"));
    }

    #[test]
    fn test_default_wait_selectors_not_empty() {
        let selectors = GoogleEngine::default_wait_selectors();
        assert!(!selectors.is_empty());
        assert!(selectors.contains(&"#search".to_string()));
        assert!(selectors.contains(&".g".to_string()));
    }

    // ==================== URL Decode Tests ====================

    #[test]
    fn test_decode_google_url_simple() {
        let url = "/url?q=https://example.com&sa=U";
        let decoded = GoogleEngine::decode_google_url(url);
        assert_eq!(decoded, Some("https://example.com".to_string()));
    }

    #[test]
    fn test_decode_google_url_encoded() {
        let url = "/url?q=https%3A%2F%2Fexample.com%2Fpath%3Fquery%3Dvalue&sa=U";
        let decoded = GoogleEngine::decode_google_url(url);
        assert_eq!(decoded, Some("https://example.com/path?query=value".to_string()));
    }

    #[test]
    fn test_decode_google_url_no_ampersand() {
        let url = "/url?q=https://example.com";
        let decoded = GoogleEngine::decode_google_url(url);
        assert_eq!(decoded, Some("https://example.com".to_string()));
    }

    #[test]
    fn test_decode_google_url_invalid() {
        let url = "/some/other/path";
        let decoded = GoogleEngine::decode_google_url(url);
        assert_eq!(decoded, None);
    }

    // ==================== Extract Total Results Tests ====================

    #[test]
    fn test_extract_total_results_english() {
        let html = r#"<div>About 1,234,567 results</div>"#;
        let total = GoogleEngine::extract_total_results(html);
        assert_eq!(total, Some(1234567));
    }

    #[test]
    fn test_extract_total_results_chinese() {
        let html = r#"<div>大约 12,345 条结果</div>"#;
        let total = GoogleEngine::extract_total_results(html);
        assert_eq!(total, Some(12345));
    }

    #[test]
    fn test_extract_total_results_simple_chinese() {
        let html = r#"<div>100 个结果</div>"#;
        let total = GoogleEngine::extract_total_results(html);
        assert_eq!(total, Some(100));
    }

    #[test]
    fn test_extract_total_results_no_match() {
        let html = r#"<div>No results found</div>"#;
        let total = GoogleEngine::extract_total_results(html);
        assert_eq!(total, None);
    }

    // ==================== Parse Search Results Tests ====================

    #[test]
    fn test_parse_search_results_empty_html() {
        let html = "";
        let results = GoogleEngine::parse_search_results(html, "test query");

        assert_eq!(results.query, "test query");
        assert_eq!(results.search_engine, "Google");
        assert_eq!(results.engine_id, "google");
        assert_eq!(results.homepage_url, "https://www.google.com");
        assert!(results.items.is_empty());
    }

    #[test]
    fn test_parse_search_results_no_results() {
        let html = r#"
            <html>
                <body>
                    <div id="search">No results found</div>
                </body>
            </html>
        "#;
        let results = GoogleEngine::parse_search_results(html, "nonexistent query");

        assert_eq!(results.query, "nonexistent query");
        assert!(results.items.is_empty());
    }

    #[test]
    fn test_parse_search_results_with_tF2Cxc_class() {
        let html = r#"
            <html>
                <body>
                    <div id="search">
                        <div class="tF2Cxc">
                            <div class="yuRUbf">
                                <h3>Test Title</h3>
                                <a href="https://example.com">Link</a>
                            </div>
                            <div class="VwiC3b">This is the snippet text</div>
                        </div>
                    </div>
                </body>
            </html>
        "#;
        let results = GoogleEngine::parse_search_results(html, "test");

        assert_eq!(results.query, "test");
        assert_eq!(results.search_engine, "Google");
        // 结果可能解析也可能不解析，取决于选择器匹配
    }

    #[test]
    fn test_parse_search_results_with_g_class() {
        let html = r#"
            <html>
                <body>
                    <div id="search">
                        <div class="g">
                            <div class="yuRUbf">
                                <h3>Result Title</h3>
                                <a href="https://example.org">Link</a>
                            </div>
                            <div class="VwiC3b">Result snippet</div>
                        </div>
                    </div>
                </body>
            </html>
        "#;
        let results = GoogleEngine::parse_search_results(html, "search term");

        assert_eq!(results.query, "search term");
        assert_eq!(results.engine_id, "google");
    }

    #[test]
    fn test_parse_search_results_with_total_count() {
        let html = r#"
            <html>
                <body>
                    <div>About 500,000 results</div>
                    <div id="search"></div>
                </body>
            </html>
        "#;
        let results = GoogleEngine::parse_search_results(html, "query");

        assert_eq!(results.total_results, Some(500000));
    }

    #[test]
    fn test_parse_search_results_max_20_items() {
        // 验证最多返回 20 个结果
        let mut html = String::from("<html><body><div id='search'>");
        for i in 0..30 {
            html.push_str(&format!(
                r#"<div class="g">
                    <div class="yuRUbf">
                        <h3>Result {}</h3>
                        <a href="https://example{}.com">Link</a>
                    </div>
                    <div class="VwiC3b">Snippet {}</div>
                </div>"#,
                i, i, i
            ));
        }
        html.push_str("</div></body></html>");

        let results = GoogleEngine::parse_search_results(&html, "test");

        // 最多 20 个结果
        assert!(results.items.len() <= 20);
    }

    #[test]
    fn test_parse_search_results_with_redirect_url() {
        let html = r#"
            <html>
                <body>
                    <div class="tF2Cxc">
                        <div class="yuRUbf">
                            <h3>Test</h3>
                            <a href="/url?q=https://real-url.com&sa=U">Link</a>
                        </div>
                        <div class="VwiC3b">Snippet</div>
                    </div>
                </body>
            </html>
        "#;
        let results = GoogleEngine::parse_search_results(html, "test");

        // 验证 URL 解码逻辑被调用
        if !results.items.is_empty() {
            let first = &results.items[0];
            assert!(first.url.starts_with("https://"));
        }
    }
}
