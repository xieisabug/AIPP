use crate::mcp::builtin_mcp::search::types::{SearchItem, SearchResults};
use scraper::{Html, Selector};

/// DuckDuckGo搜索引擎实现
pub struct DuckDuckGoEngine;

impl DuckDuckGoEngine {
    pub fn display_name() -> &'static str {
        "DuckDuckGo"
    }
    
    pub fn homepage_url() -> &'static str {
        "https://duckduckgo.com"
    }
    
    pub fn search_input_selectors() -> Vec<&'static str> {
        vec![
            "#search_form_input",
            "input[name='q']",
            "#searchbox_input", 
            ".js-search-input",
            "input[placeholder*='搜索']",
            "input[placeholder*='Search']",
        ]
    }
    
    pub fn search_button_selectors() -> Vec<&'static str> {
        vec![
            "input[type='submit']",
            "#search_button_homepage",
            ".search-wrap__button",
        ]
    }
    
    pub fn default_wait_selectors() -> Vec<String> {
        vec![
            "#links".to_string(),
            ".results".to_string(),
            ".result".to_string(),
            "#web_content".to_string(),
        ]
    }
    
    
    /// 解析DuckDuckGo搜索结果HTML，提取结构化信息（HTML解析器版）
    pub fn parse_search_results(html: &str, query: &str) -> SearchResults {
        let mut items = Vec::new();
        let document = Html::parse_document(html);

        // 结果卡片选择器（DuckDuckGo通常使用 .result 类）
        let selectors = [
            Selector::parse("div.result").ok(),
            Selector::parse("article.result").ok(),
        ];

        let mut rank = 1usize;
        for sel in selectors.iter().flatten() {
            for card in document.select(sel) {
                if let Some(item) = Self::parse_card_element(card, rank) {
                    items.push(item);
                    rank += 1;
                    if items.len() >= 20 { break; }
                }
            }
            if !items.is_empty() { break; }
        }

        SearchResults {
            query: query.to_string(),
            search_engine: Self::display_name().to_string(),
            engine_id: "duckduckgo".to_string(),
            homepage_url: Self::homepage_url().to_string(),
            items,
            total_results: None, // DuckDuckGo通常不显示结果总数
            search_time_ms: None,
        }
    }
    
    /// 从结果卡片元素中抽取一个条目
    fn parse_card_element(card: scraper::ElementRef<'_>, rank: usize) -> Option<SearchItem> {
        // 标题：DuckDuckGo 通常使用 h2 a 或 .result__title 类
        let title = Self::first_text_in(card, &["h2 a", "h3 a", "a.result__title", "h2", "h3"])
            .unwrap_or_else(|| format!("DuckDuckGo Result {}", rank));

        // URL：寻找标题链接
        let url = Self::first_href_in(card, &["h2 a", "h3 a", "a.result__title", "a[href]"]).unwrap_or_default();

        // 摘要：DuckDuckGo 使用 .result__snippet 类或其他描述元素
        let snippet = Self::first_text_in(card, &["span.result__snippet", "div.result__snippet", "p", "div"]).unwrap_or_default();

        if !title.trim().is_empty() && !url.trim().is_empty() {
            Some(SearchItem {
                title: title.trim().to_string(),
                url,
                snippet: snippet.trim().to_string(),
                rank,
                display_url: None,
            })
        } else {
            None
        }
    }

    /// 在元素内按给定选择器列表找到首个文本
    fn first_text_in(root: scraper::ElementRef<'_>, selectors: &[&str]) -> Option<String> {
        for sel in selectors {
            if let Ok(selector) = Selector::parse(sel) {
                if let Some(node) = root.select(&selector).next() {
                    let text = node.text().collect::<String>();
                    let text = text.trim();
                    if !text.is_empty() { return Some(text.to_string()); }
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
        assert_eq!(DuckDuckGoEngine::display_name(), "DuckDuckGo");
    }

    #[test]
    fn test_homepage_url() {
        assert_eq!(DuckDuckGoEngine::homepage_url(), "https://duckduckgo.com");
    }

    #[test]
    fn test_search_input_selectors_not_empty() {
        let selectors = DuckDuckGoEngine::search_input_selectors();
        assert!(!selectors.is_empty());
        assert!(selectors.contains(&"#search_form_input"));
        assert!(selectors.contains(&"input[name='q']"));
    }

    #[test]
    fn test_search_button_selectors_not_empty() {
        let selectors = DuckDuckGoEngine::search_button_selectors();
        assert!(!selectors.is_empty());
        assert!(selectors.contains(&"input[type='submit']"));
    }

    #[test]
    fn test_default_wait_selectors_not_empty() {
        let selectors = DuckDuckGoEngine::default_wait_selectors();
        assert!(!selectors.is_empty());
        assert!(selectors.contains(&"#links".to_string()));
        assert!(selectors.contains(&".result".to_string()));
    }

    // ==================== Parse Search Results Tests ====================

    #[test]
    fn test_parse_search_results_empty_html() {
        let html = "";
        let results = DuckDuckGoEngine::parse_search_results(html, "test query");

        assert_eq!(results.query, "test query");
        assert_eq!(results.search_engine, "DuckDuckGo");
        assert_eq!(results.engine_id, "duckduckgo");
        assert_eq!(results.homepage_url, "https://duckduckgo.com");
        assert!(results.items.is_empty());
        assert!(results.total_results.is_none()); // DuckDuckGo 不显示结果总数
    }

    #[test]
    fn test_parse_search_results_no_results() {
        let html = r#"
            <html>
                <body>
                    <div id="links">No results found</div>
                </body>
            </html>
        "#;
        let results = DuckDuckGoEngine::parse_search_results(html, "nonexistent query");

        assert_eq!(results.query, "nonexistent query");
        assert!(results.items.is_empty());
    }

    #[test]
    fn test_parse_search_results_with_result_div() {
        let html = r#"
            <html>
                <body>
                    <div id="links">
                        <div class="result">
                            <h2><a href="https://example.com">Test Title</a></h2>
                            <span class="result__snippet">This is the snippet</span>
                        </div>
                    </div>
                </body>
            </html>
        "#;
        let results = DuckDuckGoEngine::parse_search_results(html, "test");

        assert_eq!(results.query, "test");
        assert_eq!(results.search_engine, "DuckDuckGo");
        assert!(!results.items.is_empty());
        
        let first = &results.items[0];
        assert_eq!(first.title, "Test Title");
        assert_eq!(first.url, "https://example.com");
    }

    #[test]
    fn test_parse_search_results_with_article_result() {
        let html = r#"
            <html>
                <body>
                    <div id="links">
                        <article class="result">
                            <h3><a href="https://example.org">Article Title</a></h3>
                            <div class="result__snippet">Article description</div>
                        </article>
                    </div>
                </body>
            </html>
        "#;
        let results = DuckDuckGoEngine::parse_search_results(html, "search term");

        assert_eq!(results.query, "search term");
        assert_eq!(results.engine_id, "duckduckgo");
    }

    #[test]
    fn test_parse_search_results_max_20_items() {
        let mut html = String::from("<html><body><div id='links'>");
        for i in 0..30 {
            html.push_str(&format!(
                r#"<div class="result">
                    <h2><a href="https://example{}.com">Result {}</a></h2>
                    <span class="result__snippet">Snippet {}</span>
                </div>"#,
                i, i, i
            ));
        }
        html.push_str("</div></body></html>");

        let results = DuckDuckGoEngine::parse_search_results(&html, "test");

        assert!(results.items.len() <= 20);
    }

    #[test]
    fn test_parse_search_results_no_total_results() {
        // DuckDuckGo 不显示结果总数
        let html = r#"
            <html>
                <body>
                    <div>Some random content</div>
                    <div id="links"></div>
                </body>
            </html>
        "#;
        let results = DuckDuckGoEngine::parse_search_results(html, "query");

        assert!(results.total_results.is_none());
    }

    #[test]
    fn test_parse_search_results_rank_increment() {
        let html = r#"
            <html>
                <body>
                    <div id="links">
                        <div class="result">
                            <h2><a href="https://first.com">First</a></h2>
                            <span class="result__snippet">First snippet</span>
                        </div>
                        <div class="result">
                            <h2><a href="https://second.com">Second</a></h2>
                            <span class="result__snippet">Second snippet</span>
                        </div>
                    </div>
                </body>
            </html>
        "#;
        let results = DuckDuckGoEngine::parse_search_results(html, "test");

        assert_eq!(results.items.len(), 2);
        assert_eq!(results.items[0].rank, 1);
        assert_eq!(results.items[1].rank, 2);
    }

    #[test]
    fn test_parse_search_results_no_display_url() {
        // DuckDuckGo 通常不设置 display_url
        let html = r#"
            <html>
                <body>
                    <div class="result">
                        <h2><a href="https://example.com">Title</a></h2>
                        <span class="result__snippet">Snippet</span>
                    </div>
                </body>
            </html>
        "#;
        let results = DuckDuckGoEngine::parse_search_results(html, "test");

        if !results.items.is_empty() {
            assert!(results.items[0].display_url.is_none());
        }
    }
}
