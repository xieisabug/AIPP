use serde::{Deserialize, Serialize};

/// 搜索结果类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SearchResultType {
    /// 返回原始HTML内容
    Html,
    /// 返回转换后的Markdown内容
    Markdown,
    /// 返回结构化的搜索结果项
    Items,
}

impl Default for SearchResultType {
    fn default() -> Self {
        SearchResultType::Markdown
    }
}

impl SearchResultType {
    pub fn from_str(s: Option<&str>) -> Self {
        match s {
            Some("html") => SearchResultType::Html,
            Some("markdown") => SearchResultType::Markdown,
            Some("items") => SearchResultType::Items,
            _ => SearchResultType::default(),
        }
    }
}

/// 搜索请求参数
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchRequest {
    /// 搜索关键词
    pub query: String,
    /// 期望的结果类型（默认 Markdown）
    #[serde(default)]
    pub result_type: SearchResultType,
}

/// 单个搜索结果项
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchItem {
    /// 结果标题
    pub title: String,
    /// 结果链接
    pub url: String,
    /// 结果摘要/描述
    pub snippet: String,
    /// 搜索结果排名（从1开始）
    pub rank: usize,
    /// 显示的URL（如果与实际URL不同）
    pub display_url: Option<String>,
}

/// 结构化搜索结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResults {
    /// 搜索查询
    pub query: String,
    /// 搜索引擎名称
    pub search_engine: String,
    /// 搜索引擎ID
    pub engine_id: String,
    /// 搜索引擎首页URL
    pub homepage_url: String,
    /// 结果项列表
    pub items: Vec<SearchItem>,
    /// 总结果数量（如果可获取）
    pub total_results: Option<u64>,
    /// 搜索耗时（毫秒）
    pub search_time_ms: Option<u64>,
}

/// 搜索响应统一格式
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SearchResponse {
    /// HTML内容响应
    Html {
        query: String,
        homepage_url: String,
        search_engine: String,
        engine_id: String,
        html_content: String,
        message: String,
    },
    /// Markdown内容响应
    Markdown {
        query: String,
        homepage_url: String,
        search_engine: String,
        engine_id: String,
        markdown_content: String,
        message: String,
    },
    /// 结构化结果响应（完整对象）
    Items(SearchResults),
    /// 简化的搜索结果响应（仅包含结果项数组）
    ItemsOnly(Vec<SearchItem>),
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================
    // SearchResultType Tests
    // ============================================

    #[test]
    fn test_search_result_type_default() {
        let result_type = SearchResultType::default();
        assert_eq!(result_type, SearchResultType::Markdown);
    }

    #[test]
    fn test_search_result_type_from_str_html() {
        let result = SearchResultType::from_str(Some("html"));
        assert_eq!(result, SearchResultType::Html);
    }

    #[test]
    fn test_search_result_type_from_str_markdown() {
        let result = SearchResultType::from_str(Some("markdown"));
        assert_eq!(result, SearchResultType::Markdown);
    }

    #[test]
    fn test_search_result_type_from_str_items() {
        let result = SearchResultType::from_str(Some("items"));
        assert_eq!(result, SearchResultType::Items);
    }

    #[test]
    fn test_search_result_type_from_str_none() {
        let result = SearchResultType::from_str(None);
        assert_eq!(result, SearchResultType::Markdown); // default
    }

    #[test]
    fn test_search_result_type_from_str_invalid() {
        let result = SearchResultType::from_str(Some("invalid"));
        assert_eq!(result, SearchResultType::Markdown); // default
    }

    #[test]
    fn test_search_result_type_from_str_empty() {
        let result = SearchResultType::from_str(Some(""));
        assert_eq!(result, SearchResultType::Markdown); // default
    }

    // ============================================
    // SearchRequest Tests
    // ============================================

    #[test]
    fn test_search_request_serialize() {
        let request = SearchRequest {
            query: "test query".to_string(),
            result_type: SearchResultType::Markdown,
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("test query"));
        assert!(json.contains("markdown"));
    }

    #[test]
    fn test_search_request_deserialize() {
        let json = r#"{"query": "hello world", "result_type": "items"}"#;
        let request: SearchRequest = serde_json::from_str(json).unwrap();

        assert_eq!(request.query, "hello world");
        assert_eq!(request.result_type, SearchResultType::Items);
    }

    #[test]
    fn test_search_request_deserialize_default_result_type() {
        let json = r#"{"query": "hello"}"#;
        let request: SearchRequest = serde_json::from_str(json).unwrap();

        assert_eq!(request.query, "hello");
        assert_eq!(request.result_type, SearchResultType::Html); // default
    }

    // ============================================
    // SearchItem Tests
    // ============================================

    #[test]
    fn test_search_item_serialize() {
        let item = SearchItem {
            title: "Test Title".to_string(),
            url: "https://example.com".to_string(),
            snippet: "Test snippet".to_string(),
            rank: 1,
            display_url: Some("example.com".to_string()),
        };

        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("Test Title"));
        assert!(json.contains("https://example.com"));
        assert!(json.contains("\"rank\":1"));
    }

    #[test]
    fn test_search_item_deserialize() {
        let json = r#"{
            "title": "Result",
            "url": "https://test.com",
            "snippet": "A test result",
            "rank": 3,
            "display_url": null
        }"#;

        let item: SearchItem = serde_json::from_str(json).unwrap();
        assert_eq!(item.title, "Result");
        assert_eq!(item.url, "https://test.com");
        assert_eq!(item.rank, 3);
        assert!(item.display_url.is_none());
    }

    // ============================================
    // SearchResults Tests
    // ============================================

    #[test]
    fn test_search_results_serialize() {
        let results = SearchResults {
            query: "rust programming".to_string(),
            search_engine: "Google".to_string(),
            engine_id: "google".to_string(),
            homepage_url: "https://www.google.com".to_string(),
            items: vec![SearchItem {
                title: "Rust Lang".to_string(),
                url: "https://rust-lang.org".to_string(),
                snippet: "Official Rust website".to_string(),
                rank: 1,
                display_url: None,
            }],
            total_results: Some(1000000),
            search_time_ms: Some(250),
        };

        let json = serde_json::to_string(&results).unwrap();
        assert!(json.contains("rust programming"));
        assert!(json.contains("Google"));
        assert!(json.contains("Rust Lang"));
    }

    #[test]
    fn test_search_results_empty_items() {
        let results = SearchResults {
            query: "no results".to_string(),
            search_engine: "Bing".to_string(),
            engine_id: "bing".to_string(),
            homepage_url: "https://www.bing.com".to_string(),
            items: vec![],
            total_results: Some(0),
            search_time_ms: None,
        };

        assert!(results.items.is_empty());
        assert_eq!(results.total_results, Some(0));
    }
}
