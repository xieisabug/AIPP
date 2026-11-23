use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

pub const MAX_AVATARS_PER_PLUGIN: usize = 32;
pub const MAX_ACCESSORIES_PER_PLUGIN: usize = 64;
pub const MAX_LAYERS_PER_ASSET: usize = 32;
pub const MAX_PATH_DATA_LEN: usize = 16 * 1024;
pub const MAX_ASSET_FILE_BYTES: u64 = 256 * 1024;

const ALLOWED_ACTIONS: &[&str] = &[
    "blink",
    "smile",
    "glasses",
    "hat",
    "peer",
    "blush",
    "jingle",
    "butterfly",
    "raise-brow",
    "listen",
    "straighten-crown",
    "twirl",
    "fly",
    "sprout-sway",
    "doze",
];

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VectorTransform {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotate: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale_x: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale_y: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VectorLayer {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub d: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rx: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ry: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cx: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cy: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_opacity: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke_width: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke_opacity: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke_linecap: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke_linejoin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub white_edge: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transform: Option<VectorTransform>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AvatarAnchorAsset {
    pub x: f64,
    pub y: f64,
    pub scale: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale_y: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AvatarColorLabelsAsset {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary: Option<String>,
    pub secondary: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AvatarDefaultColorsAsset {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PluginAvatarAsset {
    pub id: String,
    pub local_id: String,
    pub plugin_code: String,
    pub plugin_name: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub vector_parts: Vec<VectorLayer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artwork_scale: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eye_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eye_scale: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face_color_slot: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_colors: Option<AvatarDefaultColorsAsset>,
    pub face: AvatarAnchorAsset,
    pub crown: AvatarAnchorAsset,
    pub headphones: AvatarAnchorAsset,
    pub clip: AvatarAnchorAsset,
    pub prop_dock: AvatarAnchorAsset,
    pub color_labels: AvatarColorLabelsAsset,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PluginAccessoryAsset {
    pub id: String,
    pub local_id: String,
    pub plugin_code: String,
    pub plugin_name: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub slot: String,
    pub anchor: String,
    pub follow_gaze: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    pub preview_view_box: String,
    pub layers: Vec<VectorLayer>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PluginAvatarAssetBundle {
    pub avatars: Vec<PluginAvatarAsset>,
    pub accessories: Vec<PluginAccessoryAsset>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawAnchorsInput {
    face: Option<AvatarAnchorAsset>,
    crown: Option<AvatarAnchorAsset>,
    headphones: Option<AvatarAnchorAsset>,
    clip: Option<AvatarAnchorAsset>,
    prop_dock: Option<AvatarAnchorAsset>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawAvatarInput {
    #[serde(default)]
    schema_version: Option<u32>,
    id: String,
    label: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default, alias = "vectorParts")]
    parts: Vec<JsonValue>,
    #[serde(default)]
    artwork_scale: Option<f64>,
    #[serde(default)]
    eye_color: Option<String>,
    #[serde(default)]
    eye_scale: Option<f64>,
    #[serde(default)]
    face_color_slot: Option<String>,
    #[serde(default)]
    default_colors: Option<AvatarDefaultColorsAsset>,
    #[serde(default)]
    anchors: Option<RawAnchorsInput>,
    #[serde(default)]
    face: Option<AvatarAnchorAsset>,
    #[serde(default)]
    crown: Option<AvatarAnchorAsset>,
    #[serde(default)]
    headphones: Option<AvatarAnchorAsset>,
    #[serde(default)]
    clip: Option<AvatarAnchorAsset>,
    #[serde(default)]
    prop_dock: Option<AvatarAnchorAsset>,
    #[serde(default)]
    color_labels: Option<AvatarColorLabelsAsset>,
    #[serde(default)]
    capabilities: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawAccessoryInput {
    #[serde(default)]
    schema_version: Option<u32>,
    id: String,
    label: String,
    #[serde(default)]
    description: Option<String>,
    slot: String,
    #[serde(default)]
    anchor: Option<String>,
    #[serde(default)]
    follow_gaze: Option<bool>,
    #[serde(default)]
    action: Option<String>,
    preview_view_box: String,
    #[serde(default)]
    layers: Vec<JsonValue>,
}

fn is_valid_local_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 48 {
        return false;
    }
    if !matches!(bytes[0], b'a'..=b'z' | b'0'..=b'9') {
        return false;
    }
    bytes
        .iter()
        .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_'))
}

fn is_finite_in_range(v: f64, min: f64, max: f64) -> bool {
    v.is_finite() && v >= min && v <= max
}

fn is_hex_color(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 7 || bytes[0] != b'#' {
        return false;
    }
    bytes[1..].iter().all(|b| b.is_ascii_hexdigit())
}

fn normalize_color_token(raw: &str, allow_slots: bool) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("颜色值不能为空".to_string());
    }
    let lower = trimmed.to_ascii_lowercase();
    if allow_slots
        && matches!(
            lower.as_str(),
            "primary" | "secondary" | "white" | "core" | "none" | "white-edge"
        )
    {
        return Ok(lower);
    }
    if is_hex_color(trimmed) {
        return Ok(trimmed.to_ascii_uppercase());
    }
    Err(format!("不支持的颜色值：{trimmed}（仅允许 #RRGGBB 或颜色槽位）"))
}

fn validate_path_data(d: &str) -> Result<String, String> {
    let trimmed = d.trim();
    if trimmed.is_empty() {
        return Err("SVG path 数据不能为空".to_string());
    }
    if trimmed.len() > MAX_PATH_DATA_LEN {
        return Err(format!("SVG path 数据过长（超过 {} 字节）", MAX_PATH_DATA_LEN));
    }
    if !trimmed.bytes().all(|b| {
        matches!(
            b,
            b'M' | b'm'
                | b'L'
                | b'l'
                | b'H'
                | b'h'
                | b'V'
                | b'v'
                | b'C'
                | b'c'
                | b'S'
                | b's'
                | b'Q'
                | b'q'
                | b'T'
                | b't'
                | b'A'
                | b'a'
                | b'Z'
                | b'z'
                | b'0'..=b'9'
                | b'.'
                | b','
                | b'+'
                | b'-'
                | b' '
                | b'\t'
                | b'\r'
                | b'\n'
                | b'e'
                | b'E'
        )
    }) {
        return Err("SVG path 包含非法字符".to_string());
    }
    Ok(trimmed.to_string())
}

fn validate_transform(transform: VectorTransform) -> Result<VectorTransform, String> {
    if let Some(x) = transform.x {
        if !is_finite_in_range(x, -2000.0, 2000.0) {
            return Err("transform.x 超出允许范围".to_string());
        }
    }
    if let Some(y) = transform.y {
        if !is_finite_in_range(y, -2000.0, 2000.0) {
            return Err("transform.y 超出允许范围".to_string());
        }
    }
    if let Some(rotate) = transform.rotate {
        if !is_finite_in_range(rotate, -360.0, 360.0) {
            return Err("transform.rotate 超出允许范围".to_string());
        }
    }
    for (name, scale) in [
        ("scale", transform.scale),
        ("scaleX", transform.scale_x),
        ("scaleY", transform.scale_y),
    ] {
        if let Some(v) = scale {
            if !is_finite_in_range(v, -10.0, 10.0) {
                return Err(format!("transform.{name} 超出允许范围"));
            }
        }
    }
    Ok(transform)
}

fn parse_vector_layer(raw: &JsonValue, index: usize) -> Result<VectorLayer, String> {
    let obj = raw
        .as_object()
        .ok_or_else(|| format!("图层 #{index} 必须是 JSON 对象"))?;

    // 兼容经典 avatarDefinition.parts 简写：{ color, path, outlineWidth?, whiteEdge? }
    let mut layer: VectorLayer = if obj.get("type").is_none() && obj.get("path").is_some() {
        let path = obj
            .get("path")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| format!("图层 #{index} 的 path 必须是字符串"))?;
        let color = obj
            .get("color")
            .and_then(JsonValue::as_str)
            .unwrap_or("secondary");
        let outline_width = obj.get("outlineWidth").and_then(JsonValue::as_f64);
        let white_edge = obj.get("whiteEdge").and_then(JsonValue::as_bool);
        let stroke = obj
            .get("stroke")
            .and_then(JsonValue::as_str)
            .map(str::to_string)
            .or_else(|| outline_width.filter(|w| *w > 0.0).map(|_| "secondary".to_string()));
        VectorLayer {
            kind: "path".to_string(),
            d: Some(path.to_string()),
            x: None,
            y: None,
            width: None,
            height: None,
            rx: None,
            ry: None,
            cx: None,
            cy: None,
            r: None,
            fill: Some(color.to_string()),
            fill_opacity: obj.get("fillOpacity").and_then(JsonValue::as_f64),
            stroke,
            stroke_width: outline_width.or_else(|| obj.get("strokeWidth").and_then(JsonValue::as_f64)),
            stroke_opacity: obj.get("strokeOpacity").and_then(JsonValue::as_f64),
            stroke_linecap: obj.get("strokeLinecap").and_then(JsonValue::as_str).map(str::to_string),
            stroke_linejoin: obj
                .get("strokeLinejoin")
                .and_then(JsonValue::as_str)
                .map(str::to_string)
                .or_else(|| Some("round".to_string())),
            opacity: obj.get("opacity").and_then(JsonValue::as_f64),
            white_edge,
            transform: obj
                .get("transform")
                .map(|v| serde_json::from_value::<VectorTransform>(v.clone()))
                .transpose()
                .map_err(|e| format!("图层 #{index} transform 解析失败：{e}"))?,
        }
    } else {
        serde_json::from_value::<VectorLayer>(raw.clone())
            .map_err(|e| format!("图层 #{index} 解析失败：{e}"))?
    };

    layer.kind = layer.kind.trim().to_ascii_lowercase();
    match layer.kind.as_str() {
        "path" => {
            let d = layer
                .d
                .as_deref()
                .ok_or_else(|| format!("path 图层 #{index} 缺少 d 字段"))?;
            layer.d = Some(validate_path_data(d)?);
        }
        "rect" => {
            let x = layer.x.unwrap_or(0.0);
            let y = layer.y.unwrap_or(0.0);
            let width = layer
                .width
                .ok_or_else(|| format!("rect 图层 #{index} 缺少 width 字段"))?;
            let height = layer
                .height
                .ok_or_else(|| format!("rect 图层 #{index} 缺少 height 字段"))?;
            if !is_finite_in_range(x, -2000.0, 2000.0)
                || !is_finite_in_range(y, -2000.0, 2000.0)
                || !is_finite_in_range(width, 0.0, 2000.0)
                || !is_finite_in_range(height, 0.0, 2000.0)
            {
                return Err(format!("rect 图层 #{index} 尺寸超出允许范围"));
            }
            if let Some(rx) = layer.rx {
                if !is_finite_in_range(rx, 0.0, 1000.0) {
                    return Err(format!("rect 图层 #{index} rx 超出允许范围"));
                }
            }
            if let Some(ry) = layer.ry {
                if !is_finite_in_range(ry, 0.0, 1000.0) {
                    return Err(format!("rect 图层 #{index} ry 超出允许范围"));
                }
            }
            layer.x = Some(x);
            layer.y = Some(y);
        }
        "circle" => {
            let cx = layer.cx.unwrap_or(0.0);
            let cy = layer.cy.unwrap_or(0.0);
            let r = layer
                .r
                .ok_or_else(|| format!("circle 图层 #{index} 缺少 r 字段"))?;
            if !is_finite_in_range(cx, -2000.0, 2000.0)
                || !is_finite_in_range(cy, -2000.0, 2000.0)
                || !is_finite_in_range(r, 0.0, 2000.0)
            {
                return Err(format!("circle 图层 #{index} 参数超出允许范围"));
            }
            layer.cx = Some(cx);
            layer.cy = Some(cy);
        }
        "ellipse" => {
            let cx = layer.cx.unwrap_or(0.0);
            let cy = layer.cy.unwrap_or(0.0);
            let rx = layer
                .rx
                .ok_or_else(|| format!("ellipse 图层 #{index} 缺少 rx 字段"))?;
            let ry = layer
                .ry
                .ok_or_else(|| format!("ellipse 图层 #{index} 缺少 ry 字段"))?;
            if !is_finite_in_range(cx, -2000.0, 2000.0)
                || !is_finite_in_range(cy, -2000.0, 2000.0)
                || !is_finite_in_range(rx, 0.0, 2000.0)
                || !is_finite_in_range(ry, 0.0, 2000.0)
            {
                return Err(format!("ellipse 图层 #{index} 参数超出允许范围"));
            }
            layer.cx = Some(cx);
            layer.cy = Some(cy);
        }
        other => {
            return Err(format!(
                "图层 #{index} 不支持的图形类型 '{other}'（仅允许 path/rect/circle/ellipse）"
            ));
        }
    }

    if let Some(fill) = layer.fill.as_deref() {
        layer.fill = Some(normalize_color_token(fill, true)?);
    }
    if let Some(stroke) = layer.stroke.as_deref() {
        layer.stroke = Some(normalize_color_token(stroke, true)?);
    }
    for (name, val) in [
        ("opacity", layer.opacity),
        ("fillOpacity", layer.fill_opacity),
        ("strokeOpacity", layer.stroke_opacity),
    ] {
        if let Some(v) = val {
            if !is_finite_in_range(v, 0.0, 1.0) {
                return Err(format!("图层 #{index} {name} 必须在 0~1 之间"));
            }
        }
    }
    if let Some(sw) = layer.stroke_width {
        if !is_finite_in_range(sw, 0.0, 120.0) {
            return Err(format!("图层 #{index} strokeWidth 超出允许范围"));
        }
    }
    if let Some(cap) = layer.stroke_linecap.as_deref() {
        let cap = cap.trim().to_ascii_lowercase();
        if !matches!(cap.as_str(), "butt" | "round" | "square") {
            return Err(format!("图层 #{index} 不支持的 strokeLinecap: {cap}"));
        }
        layer.stroke_linecap = Some(cap);
    }
    if let Some(join) = layer.stroke_linejoin.as_deref() {
        let join = join.trim().to_ascii_lowercase();
        if !matches!(join.as_str(), "miter" | "round" | "bevel") {
            return Err(format!("图层 #{index} 不支持的 strokeLinejoin: {join}"));
        }
        layer.stroke_linejoin = Some(join);
    }
    if let Some(transform) = layer.transform {
        layer.transform = Some(validate_transform(transform)?);
    }

    Ok(layer)
}

fn validate_anchor(name: &str, anchor: AvatarAnchorAsset) -> Result<AvatarAnchorAsset, String> {
    if !is_finite_in_range(anchor.x, -500.0, 1000.0)
        || !is_finite_in_range(anchor.y, -500.0, 1000.0)
        || !is_finite_in_range(anchor.scale, 0.1, 4.0)
    {
        return Err(format!("锚点 {name} 的坐标或缩放超出允许范围"));
    }
    if let Some(sy) = anchor.scale_y {
        if !is_finite_in_range(sy, 0.1, 4.0) {
            return Err(format!("锚点 {name}.scaleY 超出允许范围"));
        }
    }
    if let Some(rot) = anchor.rotation {
        if !is_finite_in_range(rot, -180.0, 180.0) {
            return Err(format!("锚点 {name}.rotation 超出允许范围"));
        }
    }
    Ok(anchor)
}

fn validate_view_box(raw: &str) -> Result<String, String> {
    let parts: Vec<f64> = raw
        .split(|c: char| c.is_ascii_whitespace() || c == ',')
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<f64>())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| format!("previewViewBox 格式无效：{raw}"))?;
    if parts.len() != 4 {
        return Err(format!("previewViewBox 必须包含 4 个数字：{raw}"));
    }
    if !is_finite_in_range(parts[0], -2000.0, 2000.0)
        || !is_finite_in_range(parts[1], -2000.0, 2000.0)
        || !is_finite_in_range(parts[2], 1.0, 4000.0)
        || !is_finite_in_range(parts[3], 1.0, 4000.0)
    {
        return Err(format!("previewViewBox 数值超出允许范围：{raw}"));
    }
    Ok(format!("{} {} {} {}", parts[0], parts[1], parts[2], parts[3]))
}

fn validate_relative_asset_path(plugin_dir: &Path, relative: &str) -> Result<PathBuf, String> {
    let trimmed = relative.trim();
    if trimmed.is_empty() {
        return Err("资产路径不能为空".to_string());
    }
    let rel_path = Path::new(trimmed);
    if rel_path.is_absolute() {
        return Err(format!("资产路径不能为绝对路径：{trimmed}"));
    }
    for component in rel_path.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            _ => return Err(format!("资产路径包含非法片段：{trimmed}")),
        }
    }
    let full_path = plugin_dir.join(rel_path);
    let meta = fs::metadata(&full_path)
        .map_err(|_| format!("插件资产文件不存在：{trimmed}"))?;
    if !meta.is_file() {
        return Err(format!("插件资产路径不是文件：{trimmed}"));
    }
    if meta.len() > MAX_ASSET_FILE_BYTES {
        return Err(format!(
            "插件资产文件过大：{trimmed}（上限 {} KiB）",
            MAX_ASSET_FILE_BYTES / 1024
        ));
    }
    let canonical_root = plugin_dir
        .canonicalize()
        .map_err(|e| format!("无法解析插件目录：{e}"))?;
    let canonical_file = full_path
        .canonicalize()
        .map_err(|e| format!("无法解析资产路径 {trimmed}：{e}"))?;
    if !canonical_file.starts_with(&canonical_root) {
        return Err(format!("插件资产路径越界：{trimmed}"));
    }
    Ok(canonical_file)
}

fn resolve_asset_json_entries(
    plugin_dir: &Path,
    items: &[JsonValue],
    kind_label: &str,
) -> Result<Vec<(String, JsonValue)>, Vec<String>> {
    let mut resolved = Vec::new();
    let mut errors = Vec::new();

    for (index, item) in items.iter().enumerate() {
        if let Some(rel_path) = item.as_str() {
            match validate_relative_asset_path(plugin_dir, rel_path) {
                Ok(file_path) => match fs::read_to_string(&file_path) {
                    Ok(content) => match serde_json::from_str::<JsonValue>(&content) {
                        Ok(JsonValue::Array(arr)) => {
                            for (arr_idx, entry) in arr.into_iter().enumerate() {
                                resolved.push((format!("{rel_path}[{arr_idx}]"), entry));
                            }
                        }
                        Ok(value) => resolved.push((rel_path.to_string(), value)),
                        Err(e) => errors.push(format!("{kind_label} 文件 {rel_path} JSON 解析失败：{e}")),
                    },
                    Err(e) => errors.push(format!("读取 {kind_label} 文件 {rel_path} 失败：{e}")),
                },
                Err(e) => errors.push(e),
            }
        } else if item.is_object() {
            resolved.push((format!("inline[{index}]"), item.clone()));
        } else {
            errors.push(format!("{kind_label}[{index}] 必须是相对路径字符串或 JSON 对象"));
        }
    }

    if errors.is_empty() {
        Ok(resolved)
    } else {
        Err(errors)
    }
}

fn parse_avatar_asset(
    raw: JsonValue,
    source_label: &str,
    plugin_code: &str,
    plugin_name: &str,
) -> Result<PluginAvatarAsset, String> {
    let input: RawAvatarInput = serde_json::from_value(raw)
        .map_err(|e| format!("形象 {source_label} 结构无效：{e}"))?;

    if let Some(ver) = input.schema_version {
        if ver != 1 {
            return Err(format!("形象 {source_label} 不支持的 schemaVersion: {ver}"));
        }
    }
    let local_id = input.id.trim().to_ascii_lowercase();
    if !is_valid_local_id(&local_id) {
        return Err(format!(
            "形象 {source_label} 的 id '{}' 格式无效（仅允许小写字母、数字、-、_，长度 1~48）",
            input.id
        ));
    }
    let label = input.label.trim().to_string();
    if label.is_empty() || label.chars().count() > 32 {
        return Err(format!("形象 {source_label} 的 label 不能为空且不超过 32 字"));
    }
    if input.parts.is_empty() || input.parts.len() > MAX_LAYERS_PER_ASSET {
        return Err(format!(
            "形象 {source_label} 的 parts 数量必须在 1~{} 之间",
            MAX_LAYERS_PER_ASSET
        ));
    }

    let mut vector_parts = Vec::with_capacity(input.parts.len());
    for (idx, part) in input.parts.iter().enumerate() {
        vector_parts.push(
            parse_vector_layer(part, idx)
                .map_err(|e| format!("形象 {source_label}：{e}"))?,
        );
    }

    if let Some(scale) = input.artwork_scale {
        if !is_finite_in_range(scale, 0.25, 3.0) {
            return Err(format!("形象 {source_label} 的 artworkScale 超出允许范围"));
        }
    }
    let eye_color = input
        .eye_color
        .as_deref()
        .map(|c| normalize_color_token(c, false))
        .transpose()
        .map_err(|e| format!("形象 {source_label} eyeColor 无效：{e}"))?;
    if let Some(scale) = input.eye_scale {
        if !is_finite_in_range(scale, 0.3, 2.5) {
            return Err(format!("形象 {source_label} 的 eyeScale 超出允许范围"));
        }
    }
    let face_color_slot = input
        .face_color_slot
        .as_deref()
        .map(|slot| {
            let s = slot.trim().to_ascii_lowercase();
            if matches!(s.as_str(), "primary" | "secondary" | "white") {
                Ok(s)
            } else {
                Err(format!("形象 {source_label} faceColorSlot 仅允许 primary/secondary/white"))
            }
        })
        .transpose()?;

    let default_colors = if let Some(colors) = input.default_colors {
        let primary = colors
            .primary
            .as_deref()
            .map(|c| normalize_color_token(c, false))
            .transpose()
            .map_err(|e| format!("形象 {source_label} defaultColors.primary 无效：{e}"))?;
        let secondary = colors
            .secondary
            .as_deref()
            .map(|c| normalize_color_token(c, false))
            .transpose()
            .map_err(|e| format!("形象 {source_label} defaultColors.secondary 无效：{e}"))?;
        if primary.is_some() || secondary.is_some() {
            Some(AvatarDefaultColorsAsset { primary, secondary })
        } else {
            None
        }
    } else {
        None
    };

    let anchors = input.anchors;
    let face_raw = input
        .face
        .or_else(|| anchors.as_ref().and_then(|a| a.face.clone()))
        .ok_or_else(|| format!("形象 {source_label} 缺少 face 锚点"))?;
    let face = validate_anchor("face", face_raw)
        .map_err(|e| format!("形象 {source_label}：{e}"))?;

    let crown = validate_anchor(
        "crown",
        input
            .crown
            .or_else(|| anchors.as_ref().and_then(|a| a.crown.clone()))
            .unwrap_or(AvatarAnchorAsset {
                x: face.x,
                y: 48.0,
                scale: 0.95,
                scale_y: None,
                rotation: None,
            }),
    )
    .map_err(|e| format!("形象 {source_label}：{e}"))?;

    let headphones = validate_anchor(
        "headphones",
        input
            .headphones
            .or_else(|| anchors.as_ref().and_then(|a| a.headphones.clone()))
            .unwrap_or(AvatarAnchorAsset {
                x: face.x,
                y: 38.0,
                scale: 0.95,
                scale_y: None,
                rotation: None,
            }),
    )
    .map_err(|e| format!("形象 {source_label}：{e}"))?;

    let clip = validate_anchor(
        "clip",
        input
            .clip
            .or_else(|| anchors.as_ref().and_then(|a| a.clip.clone()))
            .unwrap_or(AvatarAnchorAsset {
                x: (face.x - 105.0).clamp(40.0, 470.0),
                y: 72.0,
                scale: 0.95,
                scale_y: None,
                rotation: Some(-24.0),
            }),
    )
    .map_err(|e| format!("形象 {source_label}：{e}"))?;

    let prop_dock = validate_anchor(
        "propDock",
        input
            .prop_dock
            .or_else(|| anchors.as_ref().and_then(|a| a.prop_dock.clone()))
            .unwrap_or(AvatarAnchorAsset {
                x: 410.0,
                y: 350.0,
                scale: 0.8,
                scale_y: None,
                rotation: Some(-24.0),
            }),
    )
    .map_err(|e| format!("形象 {source_label}：{e}"))?;

    let uses_primary = vector_parts.iter().any(|layer| {
        layer.fill.as_deref() == Some("primary") || layer.stroke.as_deref() == Some("primary")
    });
    let color_labels = match input.color_labels {
        Some(labels) => {
            let secondary = labels.secondary.trim().to_string();
            if secondary.is_empty() {
                return Err(format!("形象 {source_label} colorLabels.secondary 不能为空"));
            }
            let primary = labels
                .primary
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty());
            AvatarColorLabelsAsset { primary, secondary }
        }
        None => AvatarColorLabelsAsset {
            primary: uses_primary.then(|| "主色".to_string()),
            secondary: "身体".to_string(),
        },
    };

    let capabilities = match input.capabilities {
        Some(caps) => {
            let mut normalized = Vec::new();
            for cap in caps {
                let c = cap.trim().to_ascii_lowercase();
                if !matches!(c.as_str(), "eyes" | "headwear") {
                    return Err(format!("形象 {source_label} 不支持的 capability: {c}"));
                }
                if !normalized.contains(&c) {
                    normalized.push(c);
                }
            }
            normalized
        }
        None => vec!["eyes".to_string(), "headwear".to_string()],
    };

    Ok(PluginAvatarAsset {
        id: format!("{plugin_code}:{local_id}"),
        local_id,
        plugin_code: plugin_code.to_string(),
        plugin_name: plugin_name.to_string(),
        label,
        description: input.description.map(|d| d.trim().to_string()).filter(|d| !d.is_empty()),
        vector_parts,
        artwork_scale: input.artwork_scale,
        eye_color,
        eye_scale: input.eye_scale,
        face_color_slot,
        default_colors,
        face,
        crown,
        headphones,
        clip,
        prop_dock,
        color_labels,
        capabilities,
    })
}

fn parse_accessory_asset(
    raw: JsonValue,
    source_label: &str,
    plugin_code: &str,
    plugin_name: &str,
) -> Result<PluginAccessoryAsset, String> {
    let input: RawAccessoryInput = serde_json::from_value(raw)
        .map_err(|e| format!("配饰 {source_label} 结构无效：{e}"))?;

    if let Some(ver) = input.schema_version {
        if ver != 1 {
            return Err(format!("配饰 {source_label} 不支持的 schemaVersion: {ver}"));
        }
    }
    let local_id = input.id.trim().to_ascii_lowercase();
    if !is_valid_local_id(&local_id) {
        return Err(format!(
            "配饰 {source_label} 的 id '{}' 格式无效（仅允许小写字母、数字、-、_，长度 1~48）",
            input.id
        ));
    }
    let label = input.label.trim().to_string();
    if label.is_empty() || label.chars().count() > 32 {
        return Err(format!("配饰 {source_label} 的 label 不能为空且不超过 32 字"));
    }

    let slot = input.slot.trim().to_ascii_lowercase();
    if !matches!(slot.as_str(), "eyes" | "head") {
        return Err(format!("配饰 {source_label} 的 slot 仅允许 eyes 或 head"));
    }

    let anchor = match input.anchor {
        Some(raw_anchor) => {
            let a = raw_anchor.trim().to_ascii_lowercase();
            let valid = if slot == "eyes" {
                a == "face"
            } else {
                matches!(a.as_str(), "crown" | "headphones" | "clip")
            };
            if !valid {
                return Err(format!(
                    "配饰 {source_label} 的 anchor '{a}' 与 slot '{slot}' 不匹配"
                ));
            }
            a
        }
        None => {
            if slot == "eyes" {
                "face".to_string()
            } else {
                "crown".to_string()
            }
        }
    };

    let action = match input.action {
        Some(raw_action) => {
            let normalized = raw_action.trim().to_ascii_lowercase();
            if normalized.is_empty() || normalized == "none" {
                None
            } else if ALLOWED_ACTIONS.contains(&normalized.as_str()) {
                Some(normalized)
            } else {
                return Err(format!("配饰 {source_label} 不支持的 action '{normalized}'"));
            }
        }
        None => None,
    };

    let preview_view_box = validate_view_box(&input.preview_view_box)
        .map_err(|e| format!("配饰 {source_label}：{e}"))?;

    if input.layers.is_empty() || input.layers.len() > MAX_LAYERS_PER_ASSET {
        return Err(format!(
            "配饰 {source_label} 的 layers 数量必须在 1~{} 之间",
            MAX_LAYERS_PER_ASSET
        ));
    }
    let mut layers = Vec::with_capacity(input.layers.len());
    for (idx, layer) in input.layers.iter().enumerate() {
        layers.push(
            parse_vector_layer(layer, idx)
                .map_err(|e| format!("配饰 {source_label}：{e}"))?,
        );
    }

    Ok(PluginAccessoryAsset {
        id: format!("{plugin_code}:{local_id}"),
        local_id,
        plugin_code: plugin_code.to_string(),
        plugin_name: plugin_name.to_string(),
        label,
        description: input.description.map(|d| d.trim().to_string()).filter(|d| !d.is_empty()),
        slot,
        anchor,
        follow_gaze: input.follow_gaze.unwrap_or(false),
        action,
        preview_view_box,
        layers,
    })
}

pub fn load_plugin_avatar_assets_from_dir(
    plugin_dir: &Path,
    plugin_code: &str,
    plugin_name: &str,
    avatar_items: &[JsonValue],
    accessory_items: &[JsonValue],
) -> Result<PluginAvatarAssetBundle, Vec<String>> {
    let mut errors = Vec::new();
    let mut bundle = PluginAvatarAssetBundle::default();

    let resolved_avatars = match resolve_asset_json_entries(plugin_dir, avatar_items, "形象") {
        Ok(list) => list,
        Err(errs) => {
            errors.extend(errs);
            Vec::new()
        }
    };
    let resolved_accessories = match resolve_asset_json_entries(plugin_dir, accessory_items, "配饰") {
        Ok(list) => list,
        Err(errs) => {
            errors.extend(errs);
            Vec::new()
        }
    };

    if resolved_avatars.len() > MAX_AVATARS_PER_PLUGIN {
        errors.push(format!(
            "插件 {plugin_code} 声明的形象数量（{}）超过上限 {}",
            resolved_avatars.len(),
            MAX_AVATARS_PER_PLUGIN
        ));
    }
    if resolved_accessories.len() > MAX_ACCESSORIES_PER_PLUGIN {
        errors.push(format!(
            "插件 {plugin_code} 声明的配饰数量（{}）超过上限 {}",
            resolved_accessories.len(),
            MAX_ACCESSORIES_PER_PLUGIN
        ));
    }

    let mut seen_avatar_ids = HashSet::new();
    for (source, entry) in resolved_avatars {
        match parse_avatar_asset(entry, &source, plugin_code, plugin_name) {
            Ok(asset) => {
                if !seen_avatar_ids.insert(asset.local_id.clone()) {
                    errors.push(format!("插件 {plugin_code} 存在重复形象 id：{}", asset.local_id));
                } else {
                    bundle.avatars.push(asset);
                }
            }
            Err(err) => errors.push(err),
        }
    }

    let mut seen_accessory_ids = HashSet::new();
    for (source, entry) in resolved_accessories {
        match parse_accessory_asset(entry, &source, plugin_code, plugin_name) {
            Ok(asset) => {
                if !seen_accessory_ids.insert(asset.local_id.clone()) {
                    errors.push(format!("插件 {plugin_code} 存在重复配饰 id：{}", asset.local_id));
                } else {
                    bundle.accessories.push(asset);
                }
            }
            Err(err) => errors.push(err),
        }
    }

    if errors.is_empty() {
        Ok(bundle)
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aipp_avatar_assets_{label}_{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn loads_valid_avatar_and_accessory_files() {
        let dir = temp_dir("valid");
        fs::create_dir_all(dir.join("avatars")).unwrap();
        fs::create_dir_all(dir.join("accessories")).unwrap();

        fs::write(
            dir.join("avatars/cat.json"),
            serde_json::to_string(&json!({
                "schemaVersion": 1,
                "id": "cat",
                "label": "小橘猫",
                "parts": [
                    { "color": "secondary", "path": "M100 100 H400 V400 H100 Z" },
                    { "type": "circle", "cx": 180, "cy": 260, "r": 18, "fill": "#FF8FA3", "opacity": 0.7 }
                ],
                "defaultColors": { "primary": "#FFF0B5", "secondary": "#D87855" },
                "face": { "x": 255, "y": 230, "scale": 1.1 },
                "colorLabels": { "primary": "脸部", "secondary": "身体" }
            }))
            .unwrap(),
        )
        .unwrap();

        fs::write(
            dir.join("accessories/star-glasses.json"),
            serde_json::to_string(&json!({
                "schemaVersion": 1,
                "id": "star-glasses",
                "label": "星星眼镜",
                "slot": "eyes",
                "anchor": "face",
                "action": "glasses",
                "previewViewBox": "-140 -65 280 130",
                "layers": [
                    { "type": "rect", "x": -80, "y": -30, "width": 60, "height": 50, "rx": 14, "fill": "#FFD666", "stroke": "#253449", "strokeWidth": 6 }
                ]
            }))
            .unwrap(),
        )
        .unwrap();

        let bundle = load_plugin_avatar_assets_from_dir(
            &dir,
            "cute-pets",
            "萌宠包",
            &[json!("avatars/cat.json")],
            &[json!("accessories/star-glasses.json")],
        )
        .expect("should load valid assets");

        assert_eq!(bundle.avatars.len(), 1);
        assert_eq!(bundle.avatars[0].id, "cute-pets:cat");
        assert_eq!(bundle.avatars[0].vector_parts.len(), 2);
        assert_eq!(bundle.accessories.len(), 1);
        assert_eq!(bundle.accessories[0].id, "cute-pets:star-glasses");
        assert_eq!(bundle.accessories[0].action.as_deref(), Some("glasses"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_path_traversal_and_unsafe_svg_data() {
        let dir = temp_dir("unsafe");
        let err = load_plugin_avatar_assets_from_dir(
            &dir,
            "evil",
            "Evil",
            &[json!("../outside.json")],
            &[],
        )
        .unwrap_err();
        assert!(err.iter().any(|e| e.contains("非法片段")));

        let err2 = load_plugin_avatar_assets_from_dir(
            &dir,
            "evil",
            "Evil",
            &[json!({
                "id": "bad",
                "label": "坏图形",
                "parts": [{ "type": "path", "d": "M0 0\" onclick=\"alert(1)" }],
                "face": { "x": 255, "y": 255, "scale": 1 }
            })],
            &[],
        )
        .unwrap_err();
        assert!(err2.iter().any(|e| e.contains("非法字符")));

        let err3 = load_plugin_avatar_assets_from_dir(
            &dir,
            "evil",
            "Evil",
            &[],
            &[json!({
                "id": "bad-acc",
                "label": "错槽",
                "slot": "eyes",
                "anchor": "crown",
                "previewViewBox": "-50 -50 100 100",
                "layers": [{ "type": "circle", "cx": 0, "cy": 0, "r": 10, "fill": "url(javascript:alert(1))" }]
            })],
        )
        .unwrap_err();
        assert!(err3.iter().any(|e| e.contains("不匹配")));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn bundled_example_cat_avatar_plugin_passes_validation() {
        let plugin_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../plugin/cat-avatar-plugin");
        let raw = fs::read_to_string(plugin_dir.join("plugin.json")).unwrap();
        let manifest: JsonValue = serde_json::from_str(&raw).unwrap();
        let avatars = manifest["contributions"]["avatars"].as_array().unwrap();
        let accessories = manifest["contributions"]["accessories"].as_array().unwrap();
        let bundle = load_plugin_avatar_assets_from_dir(
            &plugin_dir,
            "cat-avatar-plugin",
            "小猫形象插件",
            avatars,
            accessories,
        )
        .expect("bundled cat example plugin should pass all validations");
        assert_eq!(bundle.avatars.len(), 1);
        assert_eq!(bundle.avatars[0].id, "cat-avatar-plugin:cat");
        assert_eq!(bundle.accessories.len(), 2);
    }
}
