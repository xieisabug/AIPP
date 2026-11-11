//! 图片进入模型上下文前的统一规整：尺寸、base64 体积、格式。
//!
//! 做法参考 codex（`codex-rs/utils/image`）与 pi（`coding-agent/src/utils/image-resize-core.ts`）：
//! 已在限制内的 PNG/JPEG/GIF/WebP 原样保留字节；超限或格式不被模型接受时才解码，
//! 按比例缩小并重新编码，必要时逐步降低 JPEG 质量、再继续缩小。
//!
//! 附件在入库时规整一次（不保留原图），发送时再兜底检查历史里的旧附件。

use std::collections::VecDeque;
use std::io::Cursor;
use std::sync::{Arc, LazyLock, Mutex};

use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::imageops::FilterType;
use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader};
use sha2::{Digest, Sha256};
use tracing::{info, warn};

/// 长边上限。同时满足 Claude 多图请求的单图限制（2000px）与 OpenAI 的 2048px。
pub const MAX_DIMENSION: u32 = 2000;
/// base64 编码后的体积上限（4.5 MiB），低于 Bedrock 等渠道 5 MB 的单图限制。
pub const MAX_BASE64_BYTES: usize = 4 * 1024 * 1024 + 512 * 1024;
/// 不透明图片依次尝试的 JPEG 质量。
const JPEG_QUALITIES: [u8; 4] = [80, 70, 55, 40];
/// 所有质量都放不下时，每轮把尺寸缩到原来的 3/4 再试。
const SHRINK_FACTOR: f64 = 0.75;
/// 发送兜底的缓存条数：旧附件每轮都会重发，避免每轮重复解码缩放。
const SEND_CACHE_CAPACITY: usize = 16;

#[derive(Debug, Clone)]
pub struct PromptImage {
    pub mime: &'static str,
    pub base64: String,
    pub width: u32,
    pub height: u32,
    pub source_width: u32,
    pub source_height: u32,
    /// 是否重新编码过（false 表示原字节直接沿用）。
    pub reencoded: bool,
}

impl PromptImage {
    pub fn data_url(&self) -> String {
        format!("data:{};base64,{}", self.mime, self.base64)
    }
}

pub fn base64_len(byte_len: usize) -> usize {
    byte_len.div_ceil(3) * 4
}

/// 规整一张图片的原始字节。格式按文件内容识别，不信任扩展名或声明的 MIME。
pub fn prepare(bytes: &[u8]) -> Result<PromptImage> {
    let format = image::guess_format(bytes).map_err(|_| anyhow!("无法识别的图片格式"))?;
    let (source_width, source_height) = ImageReader::with_format(Cursor::new(bytes), format)
        .into_dimensions()
        .context("读取图片尺寸失败")?;

    if let Some(mime) = passthrough_mime(format) {
        if source_width <= MAX_DIMENSION
            && source_height <= MAX_DIMENSION
            && base64_len(bytes.len()) <= MAX_BASE64_BYTES
        {
            return Ok(PromptImage {
                mime,
                base64: STANDARD.encode(bytes),
                width: source_width,
                height: source_height,
                source_width,
                source_height,
                reencoded: false,
            });
        }
    }

    let source = decode_oriented(bytes, format)?;
    let (mut width, mut height) = fit_within(source.width(), source.height(), MAX_DIMENSION);
    loop {
        // 每轮都从摆正后的原图缩放，避免多次缩放累积模糊。
        let resized;
        let candidate = if (width, height) == (source.width(), source.height()) {
            &source
        } else {
            resized = source.resize_exact(width, height, FilterType::Triangle);
            &resized
        };
        if let Some((mime, encoded)) = encode_within_budget(candidate, format)? {
            return Ok(PromptImage {
                mime,
                base64: STANDARD.encode(encoded),
                width,
                height,
                source_width,
                source_height,
                reencoded: true,
            });
        }
        if width == 1 && height == 1 {
            return Err(anyhow!("图片无法压缩到模型可接受的大小"));
        }
        width = ((f64::from(width) * SHRINK_FACTOR) as u32).max(1);
        height = ((f64::from(height) * SHRINK_FACTOR) as u32).max(1);
    }
}

/// 规整 `data:<mime>;base64,<data>` 形式的图片。
pub fn prepare_data_url(data_url: &str) -> Result<PromptImage> {
    let (_, payload) = split_base64_data_url(data_url).ok_or_else(|| anyhow!("无效的图片 data URL"))?;
    let bytes = STANDARD.decode(payload.trim()).context("图片 base64 解码失败")?;
    prepare(&bytes)
}

/// 发送兜底：历史里的旧附件可能超限或 MIME 标错。需要改动时返回规整后的 (mime, base64)，
/// 已合规或无法处理时返回 None，调用方按原样发送。
pub fn fit_base64_for_send(mime: &str, payload: &str) -> Option<(String, String)> {
    let Ok(bytes) = STANDARD.decode(payload.trim()) else {
        return None;
    };
    if already_fits(mime, &bytes) {
        return None;
    }

    let key: [u8; 32] = Sha256::digest(payload.as_bytes()).into();
    if let Some(cached) = cache_get(&key) {
        return Some((cached.mime.to_string(), cached.base64.clone()));
    }
    match prepare(&bytes) {
        Ok(image) => {
            info!(
                from = %format!("{}x{} {}B", image.source_width, image.source_height, payload.len()),
                to = %format!("{}x{} {}B", image.width, image.height, image.base64.len()),
                "normalized oversized history image before sending"
            );
            let image = Arc::new(image);
            cache_put(key, image.clone());
            Some((image.mime.to_string(), image.base64.clone()))
        }
        Err(error) => {
            warn!(%error, "failed to normalize history image; sending original");
            None
        }
    }
}

/// 发送兜底的快速判断：只读文件头，不解码像素。
fn already_fits(mime: &str, bytes: &[u8]) -> bool {
    let Ok(format) = image::guess_format(bytes) else {
        return false;
    };
    let Some(actual_mime) = passthrough_mime(format) else {
        return false;
    };
    if !actual_mime.eq_ignore_ascii_case(mime.trim()) || base64_len(bytes.len()) > MAX_BASE64_BYTES {
        return false;
    }
    ImageReader::with_format(Cursor::new(bytes), format)
        .into_dimensions()
        .is_ok_and(|(width, height)| width <= MAX_DIMENSION && height <= MAX_DIMENSION)
}

fn passthrough_mime(format: ImageFormat) -> Option<&'static str> {
    match format {
        ImageFormat::Png => Some("image/png"),
        ImageFormat::Jpeg => Some("image/jpeg"),
        ImageFormat::Gif => Some("image/gif"),
        ImageFormat::WebP => Some("image/webp"),
        _ => None,
    }
}

fn split_base64_data_url(data_url: &str) -> Option<(&str, &str)> {
    let rest = data_url.strip_prefix("data:")?;
    let (header, payload) = rest.split_once(',')?;
    header
        .split(';')
        .any(|part| part.eq_ignore_ascii_case("base64"))
        .then_some((header, payload))
}

/// 解码并按 EXIF 方向摆正（重新编码会丢掉 EXIF），再统一成 8 位 RGB / RGBA / 灰度。
fn decode_oriented(bytes: &[u8], format: ImageFormat) -> Result<DynamicImage> {
    let mut decoder = ImageReader::with_format(Cursor::new(bytes), format)
        .into_decoder()
        .context("解码图片失败")?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut image = DynamicImage::from_decoder(decoder).context("解码图片失败")?;
    image.apply_orientation(orientation);

    let color = image.color();
    if color.has_alpha() {
        let rgba = image.to_rgba8();
        if rgba.pixels().any(|pixel| pixel.0[3] < u8::MAX) {
            return Ok(DynamicImage::ImageRgba8(rgba));
        }
    }
    Ok(if color.has_color() {
        DynamicImage::ImageRgb8(image.to_rgb8())
    } else {
        DynamicImage::ImageLuma8(image.to_luma8())
    })
}

/// 在当前尺寸下找一个放得进预算的编码。透明图只用 PNG；不透明图在 PNG 与 JPEG 间取更小的，
/// 都放不下再降 JPEG 质量。原图是 JPEG（照片）时跳过 PNG，PNG 几乎不可能更小。
fn encode_within_budget(
    image: &DynamicImage,
    source_format: ImageFormat,
) -> Result<Option<(&'static str, Vec<u8>)>> {
    let fits = |encoded: &Vec<u8>| base64_len(encoded.len()) <= MAX_BASE64_BYTES;

    if image.color().has_alpha() {
        let png = encode_png(image)?;
        return Ok(fits(&png).then_some(("image/png", png)));
    }

    let jpeg = encode_jpeg(image, JPEG_QUALITIES[0])?;
    if source_format != ImageFormat::Jpeg {
        let png = encode_png(image)?;
        let (smaller, larger) = if png.len() <= jpeg.len() {
            (("image/png", png), ("image/jpeg", jpeg))
        } else {
            (("image/jpeg", jpeg), ("image/png", png))
        };
        if fits(&smaller.1) {
            return Ok(Some(smaller));
        }
        if fits(&larger.1) {
            return Ok(Some(larger));
        }
    } else if fits(&jpeg) {
        return Ok(Some(("image/jpeg", jpeg)));
    }

    for quality in &JPEG_QUALITIES[1..] {
        let jpeg = encode_jpeg(image, *quality)?;
        if fits(&jpeg) {
            return Ok(Some(("image/jpeg", jpeg)));
        }
    }
    Ok(None)
}

fn encode_png(image: &DynamicImage) -> Result<Vec<u8>> {
    let mut buffer = Vec::new();
    image.write_with_encoder(PngEncoder::new(&mut buffer)).context("PNG 编码失败")?;
    Ok(buffer)
}

fn encode_jpeg(image: &DynamicImage, quality: u8) -> Result<Vec<u8>> {
    let mut buffer = Vec::new();
    image
        .write_with_encoder(JpegEncoder::new_with_quality(&mut buffer, quality))
        .context("JPEG 编码失败")?;
    Ok(buffer)
}

/// 按比例缩到长边不超过 `max`。
pub fn fit_within(width: u32, height: u32, max: u32) -> (u32, u32) {
    if width <= max && height <= max {
        return (width, height);
    }
    let scale = f64::from(max) / f64::from(width.max(height));
    (
        ((f64::from(width) * scale).round() as u32).clamp(1, max),
        ((f64::from(height) * scale).round() as u32).clamp(1, max),
    )
}

static SEND_CACHE: LazyLock<Mutex<VecDeque<([u8; 32], Arc<PromptImage>)>>> =
    LazyLock::new(|| Mutex::new(VecDeque::with_capacity(SEND_CACHE_CAPACITY)));

fn cache_get(key: &[u8; 32]) -> Option<Arc<PromptImage>> {
    let mut cache = SEND_CACHE.lock().ok()?;
    let index = cache.iter().position(|(cached, _)| cached == key)?;
    let entry = cache.remove(index)?;
    let image = entry.1.clone();
    cache.push_front(entry);
    Some(image)
}

fn cache_put(key: [u8; 32], image: Arc<PromptImage>) {
    let Ok(mut cache) = SEND_CACHE.lock() else {
        return;
    };
    cache.retain(|(cached, _)| cached != &key);
    cache.push_front((key, image));
    cache.truncate(SEND_CACHE_CAPACITY);
}
