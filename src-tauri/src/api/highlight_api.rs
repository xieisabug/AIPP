use std::collections::BTreeMap;
use std::io::{BufReader, Cursor};
use std::sync::OnceLock;

use crate::FeatureConfigState;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use syntect::dumps::{from_reader, from_uncompressed_data};
use syntect::highlighting::{Color, Theme, ThemeSet};
use syntect::html::highlighted_html_for_string;
use syntect::parsing::{Scope, SyntaxSet};
use tracing::debug;

static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
static THEME_SET: OnceLock<ThemeSet> = OnceLock::new();

#[derive(Debug, Deserialize)]
struct BatLazyThemeSet {
    themes: BTreeMap<String, BatLazyTheme>,
}

#[derive(Debug, Deserialize)]
struct BatLazyTheme {
    serialized: Vec<u8>,
}

#[derive(Debug, Serialize)]
pub struct SyntectThemeInfo {
    name: String,
    is_dark: bool,
}

fn is_zlib_compressed(data: &[u8]) -> bool {
    data.len() >= 2 && data[0] == 0x78 && matches!(data[1], 0x01 | 0x9c | 0xda)
}

fn syntax_set() -> &'static SyntaxSet {
    SYNTAX_SET.get_or_init(|| {
        let bundled = include_bytes!("../../assets/syntaxes.bin");
        match load_dump::<SyntaxSet>(bundled) {
            Ok(ss) => {
                debug!(syntax_count = ss.syntaxes().len(), "Loaded bundled syntax set");
                ss
            }
            Err((raw_error, compressed_error)) => {
                debug!(%raw_error, %compressed_error, "Failed to load bundled syntax set, using defaults");
                SyntaxSet::load_defaults_newlines()
            }
        }
    })
}

fn theme_set() -> &'static ThemeSet {
    THEME_SET.get_or_init(|| {
        let bundled = include_bytes!("../../assets/themes.bin");
        if !is_zlib_compressed(bundled) {
            match load_bat_theme_set(bundled) {
                Ok(ts) => {
                    debug!(theme_count = ts.themes.len(), "Loaded bundled bat theme set");
                    return ts;
                }
                Err(error) => {
                    debug!(%error, "Failed to load bundled bat theme set, using defaults");
                    return ThemeSet::load_defaults();
                }
            }
        }

        match load_dump::<ThemeSet>(bundled) {
            Ok(ts) => {
                debug!(theme_count = ts.themes.len(), "Loaded bundled theme set");
                ts
            }
            Err((raw_error, compressed_error)) => {
                debug!(%raw_error, %compressed_error, "Failed to load bundled theme set, using defaults");
                ThemeSet::load_defaults()
            }
        }
    })
}

fn load_dump<T: DeserializeOwned>(data: &'static [u8]) -> Result<T, (String, String)> {
    match from_uncompressed_data::<T>(data) {
        Ok(value) => Ok(value),
        Err(raw_error) => {
            let reader = BufReader::new(Cursor::new(data));
            match from_reader::<T, _>(reader) {
                Ok(value) => Ok(value),
                Err(compressed_error) => Err((raw_error.to_string(), compressed_error.to_string())),
            }
        }
    }
}

fn load_bat_theme_set(data: &'static [u8]) -> Result<ThemeSet, String> {
    let lazy_set = match from_uncompressed_data::<BatLazyThemeSet>(data) {
        Ok(value) => value,
        Err(raw_error) => {
            let reader = BufReader::new(Cursor::new(data));
            match from_reader::<BatLazyThemeSet, _>(reader) {
                Ok(value) => value,
                Err(compressed_error) => {
                    return Err(format!(
                        "raw_error={}, compressed_error={}",
                        raw_error, compressed_error
                    ));
                }
            }
        }
    };

    let mut theme_set = ThemeSet::default();
    let mut failed = 0usize;
    for (name, lazy_theme) in lazy_set.themes {
        match load_bat_theme(&lazy_theme.serialized) {
            Ok(theme) => {
                theme_set.themes.insert(name, theme);
            }
            Err(error) => {
                failed += 1;
                debug!(theme_name = %name, %error, "Failed to decode bat theme");
            }
        }
    }
    if theme_set.themes.is_empty() {
        return Err("bat theme set decoded 0 themes".to_string());
    }
    if failed > 0 {
        debug!(failed, "Some bat themes failed to decode");
    }
    Ok(theme_set)
}

fn load_bat_theme(data: &[u8]) -> Result<Theme, String> {
    let reader = BufReader::new(Cursor::new(data));
    match from_reader::<Theme, _>(reader) {
        Ok(theme) => Ok(theme),
        Err(compressed_error) => match from_uncompressed_data::<Theme>(data) {
            Ok(theme) => Ok(theme),
            Err(raw_error) => Err(format!(
                "raw_error={}, compressed_error={}",
                raw_error, compressed_error
            )),
        },
    }
}

fn theme_background_color(theme: &Theme) -> Option<Color> {
    let settings = &theme.settings;
    settings
        .background
        .or(settings.gutter)
        .or(settings.line_highlight)
        .or(settings.selection)
        .or(settings.selection_border)
        .or(settings.inactive_selection)
        .filter(|color| color.a > 0)
}

fn color_luminance(color: Color) -> f32 {
    let r = color.r as f32 / 255.0;
    let g = color.g as f32 / 255.0;
    let b = color.b as f32 / 255.0;
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

fn theme_name_hint(name: &str) -> Option<bool> {
    let lower = name.to_lowercase();
    let light_keywords = [
        "light", "day", "latte", "paper", "snow", "bright", "white", "github",
    ];
    let dark_keywords = [
        "dark", "night", "mocha", "frappe", "macchiato", "dim", "black", "midnight",
    ];
    if light_keywords.iter().any(|kw| lower.contains(kw)) {
        return Some(false);
    }
    if dark_keywords.iter().any(|kw| lower.contains(kw)) {
        return Some(true);
    }
    None
}

fn theme_is_dark(name: &str, theme: &Theme) -> bool {
    if let Some(bg) = theme_background_color(theme) {
        return color_luminance(bg) < 0.5;
    }
    if let Some(fg) = theme.settings.foreground.filter(|color| color.a > 0) {
        return color_luminance(fg) > 0.6;
    }
    theme_name_hint(name).unwrap_or(true)
}

fn is_blocked_theme_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower == "ansi" || lower.starts_with("ansi-") || lower.starts_with("base16")
}

fn pick_theme_by_name(name: &str) -> Option<&'static syntect::highlighting::Theme> {
    let ts = theme_set();
    ts.themes.get(name)
}

fn map_ui_theme_to_syntect(ui: &str, _is_dark: bool) -> Option<&'static str> {
    // Map UI theme ids to syntect built-in theme names
    match ui.to_lowercase().as_str() {
        // Map to closest defaults in syntect
        // Light themes - use light background themes
        "github" | "github-light" => Some("InspiredGitHub"), // InspiredGitHub is a light theme
        "vs" => Some("InspiredGitHub"),
        "atom-one-light" => Some("Solarized (light)"),
        "base16/github" => Some("base16-ocean.light"),
        // Dark themes - use dark background themes
        "github-dark" | "github-dark-dimmed" => Some("base16-ocean.dark"),
        "vs2015" => Some("base16-ocean.dark"),
        "atom-one-dark" | "atom-one-dark-reasonable" => Some("base16-eighties.dark"),
        _ => None,
    }
}

fn pick_theme(is_dark: bool) -> &'static syntect::highlighting::Theme {
    let ts = theme_set();
    // Prefer appropriate themes based on light/dark mode
    let candidates_dark =
        ["base16-ocean.dark", "base16-eighties.dark", "base16-mocha.dark", "Solarized (dark)"];
    let candidates_light = [
        "InspiredGitHub", // InspiredGitHub is a light theme with white background
        "Solarized (light)",
        "base16-ocean.light",
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

fn normalize_lang_token(raw: &str) -> &str {
    let token = raw.trim();
    if token.is_empty() {
        return token;
    }
    let token = token.split_whitespace().next().unwrap_or(token);
    token.split(|c| c == '{' || c == ';' || c == ',').next().unwrap_or(token)
}

fn map_lang_alias(token: &str) -> String {
    let lower = token.to_lowercase();
    match lower.as_str() {
        "ts" => "source.ts".to_string(),
        "tsx" => "source.tsx".to_string(),
        "js" => "source.js".to_string(),
        "jsx" => "source.tsx".to_string(),
        "react" => "source.tsx".to_string(),
        "vue" => "text.html.vue".to_string(),
        "py" => "source.python".to_string(),
        "rb" => "source.ruby".to_string(),
        "rs" => "source.rust".to_string(),
        "md" => "text.html.markdown".to_string(),
        "yml" => "source.yaml".to_string(),
        "sh" => "source.shell.bash".to_string(),
        "shell" => "source.shell.bash".to_string(),
        "zsh" => "source.shell.bash".to_string(),
        "ps" => "source.powershell".to_string(),
        "ps1" => "source.powershell".to_string(),
        "cs" => "source.cs".to_string(),
        "csharp" => "source.cs".to_string(),
        "cpp" => "source.c++".to_string(),
        "cxx" => "source.c++".to_string(),
        "hpp" => "source.c++".to_string(),
        "kt" => "source.Kotlin".to_string(),
        "golang" => "source.go".to_string(),
        "dockerfile" => "source.dockerfile".to_string(),
        other => other.to_string(),
    }
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
    let raw_token = normalize_lang_token(&lang);
    let mapped = map_lang_alias(raw_token);
    let token_lower = mapped.to_lowercase();
    let scope_match = Scope::new(&mapped)
        .ok()
        .and_then(|scope| ss.find_syntax_by_scope(scope));
    let syntax = scope_match
        .or_else(|| ss.find_syntax_by_token(&mapped))
        .or_else(|| ss.find_syntax_by_token(&token_lower))
        .or_else(|| ss.find_syntax_by_extension(&mapped))
        .or_else(|| ss.find_syntax_by_extension(&token_lower))
        .or_else(|| ss.find_syntax_by_name(&mapped))
        .or_else(|| ss.find_syntax_by_name(&token_lower))
        .unwrap_or_else(|| ss.find_syntax_plain_text());
    debug!(
        raw_lang = %lang,
        mapped_lang = %mapped,
        syntax_name = %syntax.name,
        syntax_scope = %syntax.scope,
        "Highlight syntax resolved"
    );

    // Use helper to generate inline-styled HTML within <pre><code> ... </code></pre>
    let html = highlighted_html_for_string(&code, ss, syntax, theme).map_err(|e| e.to_string())?;

    Ok(html)
}

#[tauri::command]
pub fn list_syntect_themes() -> Vec<SyntectThemeInfo> {
    let ts = theme_set();
    ts.themes
        .iter()
        .filter(|(name, _)| !is_blocked_theme_name(name))
        .map(|(name, theme)| SyntectThemeInfo {
            name: name.clone(),
            is_dark: theme_is_dark(name, theme),
        })
        .collect()
}
