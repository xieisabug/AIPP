use serde::{Deserialize, Serialize};
use tracing::debug;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SearchEngine {
    Google,
    Bing,
    DuckDuckGo,
    Kagi,
}

impl SearchEngine {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "google" => Some(SearchEngine::Google),
            "bing" => Some(SearchEngine::Bing),
            "duckduckgo" | "ddg" => Some(SearchEngine::DuckDuckGo),
            "kagi" => Some(SearchEngine::Kagi),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            SearchEngine::Google => "google",
            SearchEngine::Bing => "bing",
            SearchEngine::DuckDuckGo => "duckduckgo",
            SearchEngine::Kagi => "kagi",
        }
    }

    /// 获取默认的等待选择器
    pub fn default_wait_selectors(&self) -> Vec<String> {
        match self {
            SearchEngine::Google => super::engines::google::GoogleEngine::default_wait_selectors(),
            SearchEngine::Bing => super::engines::bing::BingEngine::default_wait_selectors(),
            SearchEngine::DuckDuckGo => {
                super::engines::duckduckgo::DuckDuckGoEngine::default_wait_selectors()
            }
            SearchEngine::Kagi => super::engines::kagi::KagiEngine::default_wait_selectors(),
        }
    }

    /// 获取搜索引擎的显示名称
    pub fn display_name(&self) -> &'static str {
        match self {
            SearchEngine::Google => super::engines::google::GoogleEngine::display_name(),
            SearchEngine::Bing => super::engines::bing::BingEngine::display_name(),
            SearchEngine::DuckDuckGo => {
                super::engines::duckduckgo::DuckDuckGoEngine::display_name()
            }
            SearchEngine::Kagi => super::engines::kagi::KagiEngine::display_name(),
        }
    }

    /// 获取搜索引擎的首页URL
    pub fn homepage_url(&self) -> &'static str {
        match self {
            SearchEngine::Google => super::engines::google::GoogleEngine::homepage_url(),
            SearchEngine::Bing => super::engines::bing::BingEngine::homepage_url(),
            SearchEngine::DuckDuckGo => {
                super::engines::duckduckgo::DuckDuckGoEngine::homepage_url()
            }
            SearchEngine::Kagi => super::engines::kagi::KagiEngine::homepage_url(),
        }
    }

    /// 获取搜索框选择器（优先级从高到低）
    pub fn search_input_selectors(&self) -> Vec<&'static str> {
        match self {
            SearchEngine::Google => super::engines::google::GoogleEngine::search_input_selectors(),
            SearchEngine::Bing => super::engines::bing::BingEngine::search_input_selectors(),
            SearchEngine::DuckDuckGo => {
                super::engines::duckduckgo::DuckDuckGoEngine::search_input_selectors()
            }
            SearchEngine::Kagi => super::engines::kagi::KagiEngine::search_input_selectors(),
        }
    }

    /// 获取搜索按钮选择器（优先级从高到低）
    pub fn search_button_selectors(&self) -> Vec<&'static str> {
        match self {
            SearchEngine::Google => super::engines::google::GoogleEngine::search_button_selectors(),
            SearchEngine::Bing => super::engines::bing::BingEngine::search_button_selectors(),
            SearchEngine::DuckDuckGo => {
                super::engines::duckduckgo::DuckDuckGoEngine::search_button_selectors()
            }
            SearchEngine::Kagi => super::engines::kagi::KagiEngine::search_button_selectors(),
        }
    }
}

pub struct SearchEngineManager {
    preferred_engine: Option<SearchEngine>,
}

impl SearchEngineManager {
    pub fn new(engine_config: Option<&str>) -> Self {
        let preferred_engine = engine_config.and_then(|s| SearchEngine::from_str(s));

        Self { preferred_engine }
    }

    /// 获取可用的搜索引擎（默认使用 Google）
    pub fn get_search_engine(&self) -> SearchEngine {
        // 先尝试用户配置的搜索引擎（或默认Google）
        let primary_engine = self.preferred_engine.as_ref().unwrap_or(&SearchEngine::Google);

        // TODO: 这里可以添加搜索引擎可用性检测
        // 现在先直接返回主选引擎，如果需要降级逻辑可以在这里添加
        debug!(engine = primary_engine.as_str(), "Selected primary search engine");
        primary_engine.clone()
    }

    /// 获取搜索引擎的等待选择器（用户配置优先，否则使用默认值）
    pub fn get_wait_selectors(
        &self,
        engine: &SearchEngine,
        custom_selectors: Option<&str>,
    ) -> Vec<String> {
        if let Some(custom) = custom_selectors {
            custom.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
        } else {
            engine.default_wait_selectors()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================
    // SearchEngine::from_str Tests
    // ============================================

    #[test]
    fn test_search_engine_from_str_google() {
        assert_eq!(SearchEngine::from_str("google"), Some(SearchEngine::Google));
        assert_eq!(SearchEngine::from_str("Google"), Some(SearchEngine::Google));
        assert_eq!(SearchEngine::from_str("GOOGLE"), Some(SearchEngine::Google));
    }

    #[test]
    fn test_search_engine_from_str_bing() {
        assert_eq!(SearchEngine::from_str("bing"), Some(SearchEngine::Bing));
        assert_eq!(SearchEngine::from_str("Bing"), Some(SearchEngine::Bing));
    }

    #[test]
    fn test_search_engine_from_str_duckduckgo() {
        assert_eq!(SearchEngine::from_str("duckduckgo"), Some(SearchEngine::DuckDuckGo));
        assert_eq!(SearchEngine::from_str("ddg"), Some(SearchEngine::DuckDuckGo));
        assert_eq!(SearchEngine::from_str("DuckDuckGo"), Some(SearchEngine::DuckDuckGo));
    }

    #[test]
    fn test_search_engine_from_str_kagi() {
        assert_eq!(SearchEngine::from_str("kagi"), Some(SearchEngine::Kagi));
        assert_eq!(SearchEngine::from_str("Kagi"), Some(SearchEngine::Kagi));
    }

    #[test]
    fn test_search_engine_from_str_invalid() {
        assert_eq!(SearchEngine::from_str("unknown"), None);
        assert_eq!(SearchEngine::from_str(""), None);
        assert_eq!(SearchEngine::from_str("yahoo"), None);
    }

    // ============================================
    // SearchEngine::as_str Tests
    // ============================================

    #[test]
    fn test_search_engine_as_str() {
        assert_eq!(SearchEngine::Google.as_str(), "google");
        assert_eq!(SearchEngine::Bing.as_str(), "bing");
        assert_eq!(SearchEngine::DuckDuckGo.as_str(), "duckduckgo");
        assert_eq!(SearchEngine::Kagi.as_str(), "kagi");
    }

    // ============================================
    // SearchEngine::display_name Tests
    // ============================================

    #[test]
    fn test_search_engine_display_name() {
        assert!(!SearchEngine::Google.display_name().is_empty());
        assert!(!SearchEngine::Bing.display_name().is_empty());
        assert!(!SearchEngine::DuckDuckGo.display_name().is_empty());
        assert!(!SearchEngine::Kagi.display_name().is_empty());
    }

    // ============================================
    // SearchEngine::homepage_url Tests
    // ============================================

    #[test]
    fn test_search_engine_homepage_url_google() {
        let url = SearchEngine::Google.homepage_url();
        assert!(url.contains("google"));
        assert!(url.starts_with("https://"));
    }

    #[test]
    fn test_search_engine_homepage_url_bing() {
        let url = SearchEngine::Bing.homepage_url();
        assert!(url.contains("bing"));
        assert!(url.starts_with("https://"));
    }

    #[test]
    fn test_search_engine_homepage_url_duckduckgo() {
        let url = SearchEngine::DuckDuckGo.homepage_url();
        assert!(url.contains("duckduckgo"));
        assert!(url.starts_with("https://"));
    }

    #[test]
    fn test_search_engine_homepage_url_kagi() {
        let url = SearchEngine::Kagi.homepage_url();
        assert!(url.contains("kagi"));
        assert!(url.starts_with("https://"));
    }

    // ============================================
    // SearchEngine::default_wait_selectors Tests
    // ============================================

    #[test]
    fn test_search_engine_default_wait_selectors_not_empty() {
        assert!(!SearchEngine::Google.default_wait_selectors().is_empty());
        assert!(!SearchEngine::Bing.default_wait_selectors().is_empty());
        assert!(!SearchEngine::DuckDuckGo.default_wait_selectors().is_empty());
        assert!(!SearchEngine::Kagi.default_wait_selectors().is_empty());
    }

    // ============================================
    // SearchEngine::search_input_selectors Tests
    // ============================================

    #[test]
    fn test_search_engine_search_input_selectors_not_empty() {
        assert!(!SearchEngine::Google.search_input_selectors().is_empty());
        assert!(!SearchEngine::Bing.search_input_selectors().is_empty());
        assert!(!SearchEngine::DuckDuckGo.search_input_selectors().is_empty());
        assert!(!SearchEngine::Kagi.search_input_selectors().is_empty());
    }

    // ============================================
    // SearchEngine::search_button_selectors Tests
    // ============================================

    #[test]
    fn test_search_engine_search_button_selectors_not_empty() {
        assert!(!SearchEngine::Google.search_button_selectors().is_empty());
        assert!(!SearchEngine::Bing.search_button_selectors().is_empty());
        assert!(!SearchEngine::DuckDuckGo.search_button_selectors().is_empty());
        assert!(!SearchEngine::Kagi.search_button_selectors().is_empty());
    }

    // ============================================
    // SearchEngine Serialization Tests
    // ============================================

    #[test]
    fn test_search_engine_serialization() {
        let engine = SearchEngine::Google;
        let json = serde_json::to_string(&engine).unwrap();
        let deserialized: SearchEngine = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, SearchEngine::Google);
    }

    #[test]
    fn test_search_engine_roundtrip() {
        for engine in
            [SearchEngine::Google, SearchEngine::Bing, SearchEngine::DuckDuckGo, SearchEngine::Kagi]
        {
            let json = serde_json::to_string(&engine).unwrap();
            let deserialized: SearchEngine = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized, engine);
        }
    }

    // ============================================
    // SearchEngineManager Tests
    // ============================================

    #[test]
    fn test_manager_new_with_no_config() {
        let manager = SearchEngineManager::new(None);
        let engine = manager.get_search_engine();
        assert_eq!(engine, SearchEngine::Google); // default
    }

    #[test]
    fn test_manager_new_with_google() {
        let manager = SearchEngineManager::new(Some("google"));
        let engine = manager.get_search_engine();
        assert_eq!(engine, SearchEngine::Google);
    }

    #[test]
    fn test_manager_new_with_bing() {
        let manager = SearchEngineManager::new(Some("bing"));
        let engine = manager.get_search_engine();
        assert_eq!(engine, SearchEngine::Bing);
    }

    #[test]
    fn test_manager_new_with_duckduckgo() {
        let manager = SearchEngineManager::new(Some("duckduckgo"));
        let engine = manager.get_search_engine();
        assert_eq!(engine, SearchEngine::DuckDuckGo);
    }

    #[test]
    fn test_manager_new_with_invalid_falls_back_to_google() {
        let manager = SearchEngineManager::new(Some("invalid_engine"));
        let engine = manager.get_search_engine();
        assert_eq!(engine, SearchEngine::Google); // default fallback
    }

    // ============================================
    // get_wait_selectors Tests
    // ============================================

    #[test]
    fn test_get_wait_selectors_default() {
        let manager = SearchEngineManager::new(None);
        let selectors = manager.get_wait_selectors(&SearchEngine::Google, None);
        assert!(!selectors.is_empty());
    }

    #[test]
    fn test_get_wait_selectors_custom() {
        let manager = SearchEngineManager::new(None);
        let selectors =
            manager.get_wait_selectors(&SearchEngine::Google, Some("#custom1, #custom2"));
        assert_eq!(selectors.len(), 2);
        assert!(selectors.contains(&"#custom1".to_string()));
        assert!(selectors.contains(&"#custom2".to_string()));
    }

    #[test]
    fn test_get_wait_selectors_custom_trims_whitespace() {
        let manager = SearchEngineManager::new(None);
        let selectors = manager.get_wait_selectors(&SearchEngine::Google, Some("  #a  ,  #b  "));
        assert_eq!(selectors, vec!["#a".to_string(), "#b".to_string()]);
    }

    #[test]
    fn test_get_wait_selectors_custom_filters_empty() {
        let manager = SearchEngineManager::new(None);
        let selectors = manager.get_wait_selectors(&SearchEngine::Google, Some("#a, , #b, "));
        assert_eq!(selectors.len(), 2);
        assert!(selectors.contains(&"#a".to_string()));
        assert!(selectors.contains(&"#b".to_string()));
    }

    #[test]
    fn test_get_wait_selectors_for_each_engine() {
        let manager = SearchEngineManager::new(None);

        for engine in
            [SearchEngine::Google, SearchEngine::Bing, SearchEngine::DuckDuckGo, SearchEngine::Kagi]
        {
            let selectors = manager.get_wait_selectors(&engine, None);
            assert!(!selectors.is_empty(), "Engine {:?} should have default selectors", engine);
        }
    }
}
