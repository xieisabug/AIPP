use std::sync::OnceLock;

use syntect::highlighting::ThemeSet;
use syntect::html::highlighted_html_for_string;
use syntect::parsing::SyntaxSet;
use crate::FeatureConfigState;

static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
static THEME_SET: OnceLock<ThemeSet> = OnceLock::new();

fn syntax_set() -> &'static SyntaxSet {
    SYNTAX_SET.get_or_init(|| SyntaxSet::load_defaults_newlines())
}

fn theme_set() -> &'static ThemeSet {
    THEME_SET.get_or_init(ThemeSet::load_defaults)
}

fn pick_theme_by_name(name: &str) -> Option<&'static syntect::highlighting::Theme> {
    let ts = theme_set();
    ts.themes.get(name)
}

fn map_ui_theme_to_syntect(ui: &str, _is_dark: bool) -> Option<&'static str> {
    // Map UI theme ids to syntect built-in theme names
    match ui.to_lowercase().as_str() {
        // Map to closest defaults in syntect
        // Light
        "github" | "github-light" => Some("Solarized (light)"),
        "vs" => Some("Solarized (light)"),
        "atom-one-light" => Some("Solarized (light)"),
        "base16/github" => Some("base16-ocean.light"),
        // Dark
        "github-dark" | "github-dark-dimmed" => Some("InspiredGitHub"),
        "vs2015" => Some("base16-ocean.dark"),
        "atom-one-dark" | "atom-one-dark-reasonable" => Some("base16-eighties.dark"),
        _ => None,
    }
}

fn pick_theme(is_dark: bool) -> &'static syntect::highlighting::Theme {
    let ts = theme_set();
    // Try to prefer GitHub-like themes, then fall back to base16
    let candidates_dark = [
        "InspiredGitHub", // a popular dark theme in defaults
        "base16-ocean.dark",
        "base16-eighties.dark",
        "base16-mocha.dark",
    ];
    let candidates_light = [
        "Solarized (light)",
        "base16-ocean.light",
        "base16-eighties.light",
    ];
    if is_dark {
        for name in candidates_dark.iter() {
            if let Some(theme) = ts.themes.get(*name) {
                return theme;
            }
        }
    } else {
        for name in candidates_light.iter() {
            if let Some(theme) = ts.themes.get(*name) {
                return theme;
            }
        }
    }
    // As an ultimate fallback pick any available theme to avoid panic
    ts.themes.values().next().expect("No themes available")
}

#[tauri::command]
pub fn highlight_code(
    lang: String,
    code: String,
    is_dark: bool,
    theme_hint: Option<String>,
    feature_config_state: tauri::State<'_, FeatureConfigState>,
) -> Result<String, String> {
    let ss = syntax_set();
    // Determine theme in priority:
    // 1) Explicit theme_hint that directly matches syntect theme name
    // 2) Map theme_hint UI id -> syntect theme
    // 3) Read display.code_theme_{light,dark} from feature config and map -> syntect theme
    // 4) Fallback candidates by dark/light

    // Try theme_hint
    let mut theme_ref: Option<&'static syntect::highlighting::Theme> = None;
    if let Some(ref hint) = theme_hint {
        if let Some(t) = pick_theme_by_name(hint) {
            theme_ref = Some(t);
        } else if let Some(mapped_name) = map_ui_theme_to_syntect(hint, is_dark) {
            theme_ref = pick_theme_by_name(mapped_name);
        }
    }
    // Try feature config
    if theme_ref.is_none() {
        let config_map_guard = feature_config_state.config_feature_map.blocking_lock();
        if let Some(display_map) = config_map_guard.get("display") {
            let key = if is_dark { "code_theme_dark" } else { "code_theme_light" };
            if let Some(fc) = display_map.get(key) {
                let ui_id = fc.value.as_str();
                if let Some(mapped_name) = map_ui_theme_to_syntect(ui_id, is_dark) {
                    theme_ref = pick_theme_by_name(mapped_name);
                }
            }
        }
        drop(config_map_guard);
    }
    let theme = theme_ref.unwrap_or_else(|| pick_theme(is_dark));

    // Figure out theme name for logging
    let ts = theme_set();

    // Try by token, then by extension, else plain text
    let token = lang.trim();
    let token_lower = token.to_lowercase();
    let syntax = ss
        .find_syntax_by_token(token)
        .or_else(|| ss.find_syntax_by_token(&token_lower))
        .or_else(|| ss.find_syntax_by_extension(token))
        .unwrap_or_else(|| ss.find_syntax_plain_text());

    // Use helper to generate inline-styled HTML within <pre><code> ... </code></pre>
    let html = highlighted_html_for_string(&code, ss, syntax, theme)
        .map_err(|e| e.to_string())?;

    Ok(html)
}

#[tauri::command]
pub fn list_syntect_themes() -> Vec<String> {
    let ts = theme_set();
    ts.themes.keys().cloned().collect()
}
