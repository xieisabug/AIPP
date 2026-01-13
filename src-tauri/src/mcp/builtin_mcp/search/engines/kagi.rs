use crate::mcp::builtin_mcp::search::types::{SearchItem, SearchResults};
use scraper::{Html, Selector};

/// Kagi搜索引擎实现
pub struct KagiEngine;

impl KagiEngine {
    pub fn display_name() -> &'static str {
        "Kagi"
    }

    pub fn homepage_url() -> &'static str {
        "https://kagi.com"
    }

    pub fn search_input_selectors() -> Vec<&'static str> {
        vec![
            "input[name='q']",
            "#searchInput",
            ".search-input",
            "input[placeholder*='搜索']",
            "input[placeholder*='Search']",
        ]
    }

    pub fn search_button_selectors() -> Vec<&'static str> {
        vec!["button[type='submit']", ".search-button", "input[type='submit']"]
    }

    pub fn default_wait_selectors() -> Vec<String> {
        vec![
            "._0_SRI".to_string(),
            ".search-result".to_string(),
            "#main".to_string(),
            ".__sri-title".to_string(),
        ]
    }

    /// 解析Kagi搜索结果HTML，提取结构化信息
    /// 基于实际 Kagi HTML 结构：
    /// - 主要结果卡片: div._0_SRI.search-result
    /// - 分组结果: div.__srgi (在 div.sr-group 内)
    pub fn parse_search_results(html: &str, query: &str) -> SearchResults {
        let mut items = Vec::new();
        let document = Html::parse_document(html);
        let mut rank = 1usize;

        // 1. 首先解析主要搜索结果卡片 (div._0_SRI.search-result)
        if let Ok(main_result_selector) = Selector::parse("div._0_SRI.search-result") {
            for card in document.select(&main_result_selector) {
                if let Some(item) = Self::parse_main_result_card(card, rank) {
                    items.push(item);
                    rank += 1;
                }
                if items.len() >= 30 {
                    break;
                }
            }
        }

        // 2. 解析分组内的子结果 (div.__srgi)
        if let Ok(group_item_selector) = Selector::parse("div.__srgi") {
            for card in document.select(&group_item_selector) {
                if let Some(item) = Self::parse_group_item_card(card, rank) {
                    items.push(item);
                    rank += 1;
                }
                if items.len() >= 30 {
                    break;
                }
            }
        }

        SearchResults {
            query: query.to_string(),
            search_engine: Self::display_name().to_string(),
            engine_id: "kagi".to_string(),
            homepage_url: Self::homepage_url().to_string(),
            items,
            total_results: None,
            search_time_ms: None,
        }
    }

    /// 解析主要搜索结果卡片 (div._0_SRI.search-result)
    fn parse_main_result_card(card: scraper::ElementRef<'_>, rank: usize) -> Option<SearchItem> {
        // 标题：从 a.__sri_title_link 或 a._0_sri_title_link 获取
        let title = Self::first_text_in(
            card,
            &["a.__sri_title_link", "a._0_sri_title_link", "h3.__sri-title-box a", "h3 a"],
        );

        // URL：从标题链接获取 href
        let url = Self::first_href_in(
            card,
            &["a.__sri_title_link", "a._0_sri_title_link", "h3.__sri-title-box a", "a._0_URL"],
        );

        // 摘要：从 div._0_DESC.__sri-desc 获取
        let snippet = Self::first_text_in(
            card,
            &["div._0_DESC.__sri-desc", "div.__sri-desc", "div._0_DESC", "div.__sri-body"],
        );

        // 显示 URL：从 a.__sri-url 或 div.__sri_url_path_box 获取
        let display_url = Self::first_text_in(card, &["div.__sri_url_path_box", "a.__sri-url"]);

        let title = title?;
        let url = url?;

        if title.trim().is_empty() || url.trim().is_empty() {
            return None;
        }

        Some(SearchItem {
            title: title.trim().to_string(),
            url,
            snippet: snippet.unwrap_or_default().trim().to_string(),
            rank,
            display_url: display_url.map(|s| s.trim().to_string()),
        })
    }

    /// 解析分组内的子结果卡片 (div.__srgi)
    fn parse_group_item_card(card: scraper::ElementRef<'_>, rank: usize) -> Option<SearchItem> {
        // 标题：从 h3.__srgi-title a 获取
        let title =
            Self::first_text_in(card, &["h3.__srgi-title a", "h3.__srgi-title", "a._0_URL"]);

        // URL：从标题链接获取 href
        let url = Self::first_href_in(card, &["h3.__srgi-title a", "a._0_URL"]);

        // 摘要：从 div.__sri-desc 获取
        let snippet = Self::first_text_in(card, &["div.__sri-desc", "div.__sri-body"]);

        let title = title?;
        let url = url?;

        if title.trim().is_empty() || url.trim().is_empty() {
            return None;
        }

        Some(SearchItem {
            title: title.trim().to_string(),
            url,
            snippet: snippet.unwrap_or_default().trim().to_string(),
            rank,
            display_url: None,
        })
    }

    /// 在元素内按给定选择器列表找到首个文本
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
                        if href.starts_with("http") {
                            return Some(href.to_string());
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
        assert_eq!(KagiEngine::display_name(), "Kagi");
    }

    #[test]
    fn test_homepage_url() {
        assert_eq!(KagiEngine::homepage_url(), "https://kagi.com");
    }

    #[test]
    fn test_search_input_selectors_not_empty() {
        let selectors = KagiEngine::search_input_selectors();
        assert!(!selectors.is_empty());
        assert!(selectors.contains(&"input[name='q']"));
        assert!(selectors.contains(&"#searchInput"));
    }

    #[test]
    fn test_search_button_selectors_not_empty() {
        let selectors = KagiEngine::search_button_selectors();
        assert!(!selectors.is_empty());
        assert!(selectors.contains(&"button[type='submit']"));
    }

    #[test]
    fn test_default_wait_selectors_not_empty() {
        let selectors = KagiEngine::default_wait_selectors();
        assert!(!selectors.is_empty());
        assert!(selectors.contains(&"._0_SRI".to_string()));
        assert!(selectors.contains(&".search-result".to_string()));
    }

    // ==================== Parse Search Results Tests ====================

    #[test]
    fn test_parse_search_results_empty_html() {
        let html = "";
        let results = KagiEngine::parse_search_results(html, "test query");

        assert_eq!(results.query, "test query");
        assert_eq!(results.search_engine, "Kagi");
        assert_eq!(results.engine_id, "kagi");
        assert_eq!(results.homepage_url, "https://kagi.com");
        assert!(results.items.is_empty());
        assert!(results.total_results.is_none());
    }

    #[test]
    fn test_parse_search_results_no_results() {
        let html = r#"
            <html>
                <body>
                    <div id="main">No results found</div>
                </body>
            </html>
        "#;
        let results = KagiEngine::parse_search_results(html, "nonexistent query");

        assert_eq!(results.query, "nonexistent query");
        assert!(results.items.is_empty());
    }

    #[test]
    fn test_parse_search_results_with_main_result() {
        let html = r#"
            <html>
                <body>
                    <div class="_0_SRI search-result">
                        <a class="__sri_title_link" href="https://example.com">Test Title</a>
                        <div class="_0_DESC __sri-desc">This is the description</div>
                    </div>
                </body>
            </html>
        "#;
        let results = KagiEngine::parse_search_results(html, "test");

        assert_eq!(results.query, "test");
        assert_eq!(results.search_engine, "Kagi");
        assert!(!results.items.is_empty());

        let first = &results.items[0];
        assert_eq!(first.title, "Test Title");
        assert_eq!(first.url, "https://example.com");
    }

    #[test]
    fn test_parse_search_results_with_group_item() {
        let html = r#"
            <html>
                <body>
                    <div class="__srgi">
                        <h3 class="__srgi-title">
                            <a href="https://example.org">Group Item Title</a>
                        </h3>
                        <div class="__sri-desc">Group item description</div>
                    </div>
                </body>
            </html>
        "#;
        let results = KagiEngine::parse_search_results(html, "test");

        assert_eq!(results.query, "test");
        assert_eq!(results.engine_id, "kagi");
    }

    #[test]
    fn test_parse_search_results_max_30_items() {
        let mut html = String::from("<html><body>");
        for i in 0..40 {
            html.push_str(&format!(
                r#"<div class="_0_SRI search-result">
                    <a class="__sri_title_link" href="https://example{}.com">Result {}</a>
                    <div class="_0_DESC __sri-desc">Snippet {}</div>
                </div>"#,
                i, i, i
            ));
        }
        html.push_str("</body></html>");

        let results = KagiEngine::parse_search_results(&html, "test");

        // Kagi 最多返回 30 个结果
        assert!(results.items.len() <= 30);
    }

    #[test]
    fn test_parse_search_results_rank_increment() {
        let html = r#"
            <html>
                <body>
                    <div class="_0_SRI search-result">
                        <a class="__sri_title_link" href="https://first.com">First</a>
                        <div class="_0_DESC __sri-desc">First desc</div>
                    </div>
                    <div class="_0_SRI search-result">
                        <a class="__sri_title_link" href="https://second.com">Second</a>
                        <div class="_0_DESC __sri-desc">Second desc</div>
                    </div>
                </body>
            </html>
        "#;
        let results = KagiEngine::parse_search_results(html, "test");

        assert_eq!(results.items.len(), 2);
        assert_eq!(results.items[0].rank, 1);
        assert_eq!(results.items[1].rank, 2);
    }

    #[test]
    fn test_parse_search_results_no_total_results() {
        // Kagi 不显示结果总数
        let html = r#"
            <html>
                <body>
                    <div id="main">Some content</div>
                </body>
            </html>
        "#;
        let results = KagiEngine::parse_search_results(html, "query");

        assert!(results.total_results.is_none());
    }

    #[test]
    fn test_parse_search_results_with_display_url() {
        let html = r#"
            <html>
                <body>
                    <div class="_0_SRI search-result">
                        <a class="__sri_title_link" href="https://example.com">Title</a>
                        <div class="__sri_url_path_box">example.com/path</div>
                        <div class="_0_DESC __sri-desc">Description</div>
                    </div>
                </body>
            </html>
        "#;
        let results = KagiEngine::parse_search_results(html, "test");

        assert!(!results.items.is_empty());
        let first = &results.items[0];
        assert_eq!(first.display_url, Some("example.com/path".to_string()));
    }

    #[test]
    fn test_parse_search_results_mixed_main_and_group() {
        let html = r#"
            <html>
                <body>
                    <div class="_0_SRI search-result">
                        <a class="__sri_title_link" href="https://main.com">Main Result</a>
                        <div class="_0_DESC __sri-desc">Main desc</div>
                    </div>
                    <div class="__srgi">
                        <h3 class="__srgi-title">
                            <a href="https://group.com">Group Result</a>
                        </h3>
                        <div class="__sri-desc">Group desc</div>
                    </div>
                </body>
            </html>
        "#;
        let results = KagiEngine::parse_search_results(html, "test");

        // 应该解析主结果和分组结果
        assert!(!results.items.is_empty());
    }
}
