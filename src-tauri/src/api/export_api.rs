use crate::errors::AppError;
#[cfg(desktop)]
use crate::mcp::builtin_mcp::search::browser::BrowserManager;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use docx_rs::*;
#[cfg(desktop)]
use futures::StreamExt;
use image::GenericImageView;
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use regex::Regex;
use std::io::Cursor;
use std::path::PathBuf;
use std::time::Duration;
#[cfg(desktop)]
use chromiumoxide::browser::{Browser, BrowserConfig};
#[cfg(desktop)]
use chromiumoxide::cdp::browser_protocol::page::PrintToPdfParams;
#[cfg(desktop)]
use chromiumoxide::page::MediaTypeParams;

/// Markdown 转 Word (.docx) 字节流
#[tauri::command]
pub async fn markdown_to_docx(markdown: String) -> Result<Vec<u8>, AppError> {
    tokio::task::spawn_blocking(move || convert_markdown_to_docx(&markdown))
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?
}

/// Markdown 转 PDF 字节流（桌面端）
#[cfg(desktop)]
#[tauri::command]
pub async fn markdown_to_pdf(markdown: String) -> Result<Vec<u8>, AppError> {
    convert_markdown_to_pdf(&markdown).await
}

/// Markdown 转 PDF 字节流（移动端不支持）
#[cfg(not(desktop))]
#[tauri::command]
pub async fn markdown_to_pdf(_markdown: String) -> Result<Vec<u8>, AppError> {
    Err(AppError::InternalError(
        "PDF 导出暂不支持移动端".to_string(),
    ))
}

#[cfg(desktop)]
async fn convert_markdown_to_pdf(markdown: &str) -> Result<Vec<u8>, AppError> {
    let html = build_pdf_html(markdown);

    let browser_manager = BrowserManager::new(None);
    let browser_path = browser_manager
        .get_browser_path()
        .map_err(|e| AppError::InternalError(format!("无法找到浏览器: {e}")))?;

    let mut config_builder = BrowserConfig::builder()
        .no_sandbox()
        .launch_timeout(Duration::from_secs(45));
    if browser_path.exists() {
        config_builder = config_builder.chrome_executable(&browser_path);
    }

    let config = config_builder
        .build()
        .map_err(|e| AppError::InternalError(format!("构建浏览器配置失败: {e}")))?;

    let (mut browser, mut handler) = Browser::launch(config)
        .await
        .map_err(|e| AppError::InternalError(format!("启动浏览器失败: {e}")))?;

    let handler_task = tokio::spawn(async move {
        while handler.next().await.is_some() {}
    });

    let page = browser
        .new_page("about:blank")
        .await
        .map_err(|e| AppError::InternalError(format!("创建页面失败: {e}")))?;

    page.set_content(html)
        .await
        .map_err(|e| AppError::InternalError(format!("设置页面内容失败: {e}")))?;

    page.emulate_media_type(MediaTypeParams::Print)
        .await
        .map_err(|e| AppError::InternalError(format!("设置打印媒体失败: {e}")))?;

    page.evaluate_function(
        r#"
        async function() {
            const images = Array.from(document.images || []);
            if (images.length === 0) return true;
            await Promise.race([
                Promise.all(
                    images.map((img) => {
                        if (img.complete) return Promise.resolve(true);
                        return new Promise((resolve) => {
                            const done = () => resolve(true);
                            img.addEventListener("load", done, { once: true });
                            img.addEventListener("error", done, { once: true });
                        });
                    })
                ),
                new Promise((resolve) => setTimeout(resolve, 8000))
            ]);
            return true;
        }
        "#,
    )
    .await
    .map_err(|e| AppError::InternalError(format!("等待图片加载失败: {e}")))?;

    let pdf_params = PrintToPdfParams::builder()
        .print_background(true)
        .prefer_css_page_size(true)
        .margin_top(0.4)
        .margin_bottom(0.4)
        .margin_left(0.4)
        .margin_right(0.4)
        .build();

    let pdf_bytes = page
        .pdf(pdf_params)
        .await
        .map_err(|e| AppError::InternalError(format!("生成 PDF 失败: {e}")))?;

    let _ = page.close().await;
    let _ = browser.close().await;
    let _ = browser.wait().await;
    handler_task.abort();

    Ok(pdf_bytes)
}

#[cfg(desktop)]
fn build_pdf_html(markdown: &str) -> String {
    let parser_options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let parser = Parser::new_ext(markdown, parser_options);
    let mut body_html = String::new();
    pulldown_cmark::html::push_html(&mut body_html, parser);
    let body_html = inline_html_images_as_data_uri(&body_html);

    format!(
        r#"<!doctype html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <style>
    @page {{
      size: A4;
      margin: 14mm 12mm;
    }}
    html, body {{
      margin: 0;
      padding: 0;
      background: #fff;
      color: #111;
      font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, "Microsoft YaHei", sans-serif;
      font-size: 12px;
      line-height: 1.65;
      word-break: break-word;
    }}
    body {{
      padding: 0;
    }}
    h1, h2, h3, h4, h5, h6 {{
      margin: 0.9em 0 0.45em;
      line-height: 1.35;
      break-after: avoid-page;
      page-break-after: avoid;
    }}
    h1 {{ font-size: 1.6em; }}
    h2 {{ font-size: 1.35em; }}
    h3 {{ font-size: 1.2em; }}
    p {{
      margin: 0.5em 0;
      orphans: 3;
      widows: 3;
    }}
    pre, blockquote, table, ul, ol {{
      page-break-inside: avoid;
      break-inside: avoid;
    }}
    pre {{
      background: #f5f5f5;
      border: 1px solid #e5e5e5;
      border-radius: 6px;
      padding: 10px 12px;
      overflow: hidden;
      white-space: pre-wrap;
      margin: 0.75em 0;
      font-family: Consolas, Monaco, "Courier New", monospace;
      font-size: 11px;
      line-height: 1.5;
    }}
    code {{
      font-family: Consolas, Monaco, "Courier New", monospace;
      font-size: 0.95em;
    }}
    p > code, li > code {{
      background: #f5f5f5;
      border-radius: 4px;
      padding: 1px 4px;
    }}
    blockquote {{
      margin: 0.75em 0;
      padding: 0.25em 0 0.25em 0.8em;
      border-left: 3px solid #d4d4d4;
      color: #555;
    }}
    table {{
      width: 100%;
      border-collapse: collapse;
      margin: 0.75em 0;
      font-size: 11px;
    }}
    th, td {{
      border: 1px solid #e5e5e5;
      padding: 6px 8px;
      text-align: left;
      vertical-align: top;
    }}
    th {{
      background: #f8f8f8;
      font-weight: 600;
    }}
    img {{
      max-width: 100%;
      height: auto;
      border: 1px solid #e5e5e5;
      border-radius: 6px;
      margin: 8px 0;
      page-break-inside: avoid;
      break-inside: avoid;
    }}
    hr {{
      border: 0;
      border-top: 1px solid #ddd;
      margin: 12px 0;
    }}
  </style>
</head>
<body>
{body_html}
</body>
</html>"#
    )
}

#[cfg(desktop)]
fn inline_html_images_as_data_uri(html: &str) -> String {
    let img_regex = Regex::new(r#"(?is)<img([^>]*?)src="([^"]+)"([^>]*)>"#)
        .expect("invalid image regex");
    img_regex
        .replace_all(html, |caps: &regex::Captures| {
            let before = caps.get(1).map_or("", |m| m.as_str());
            let src = caps.get(2).map_or("", |m| m.as_str());
            let after = caps.get(3).map_or("", |m| m.as_str());
            if let Some(bytes) = load_markdown_image_bytes(src) {
                let mime = detect_image_mime_type(&bytes);
                let data_uri = format!("data:{mime};base64,{}", STANDARD.encode(bytes));
                format!(r#"<img{before}src="{data_uri}"{after}>"#)
            } else {
                caps.get(0).map_or("", |m| m.as_str()).to_string()
            }
        })
        .into_owned()
}

#[cfg(desktop)]
fn detect_image_mime_type(image_bytes: &[u8]) -> &'static str {
    match image::guess_format(image_bytes) {
        Ok(image::ImageFormat::Jpeg) => "image/jpeg",
        Ok(image::ImageFormat::Png) => "image/png",
        Ok(image::ImageFormat::Gif) => "image/gif",
        Ok(image::ImageFormat::WebP) => "image/webp",
        Ok(image::ImageFormat::Bmp) => "image/bmp",
        Ok(image::ImageFormat::Tiff) => "image/tiff",
        _ => "image/png",
    }
}

const DOCX_BODY_FONT_SIZE: usize = 22; // 11pt
const DOCX_CODE_FONT_SIZE: usize = 20; // 10pt
const DOCX_IMAGE_MAX_WIDTH_PX: u32 = 640;
const DOCX_EMU_PER_PX: u32 = 9525;

fn body_fonts() -> RunFonts {
    RunFonts::new()
        .ascii("Calibri")
        .hi_ansi("Calibri")
        .east_asia("Microsoft YaHei")
        .cs("Calibri")
}

fn code_fonts() -> RunFonts {
    RunFonts::new()
        .ascii("Consolas")
        .hi_ansi("Consolas")
        .east_asia("Consolas")
        .cs("Consolas")
}

fn body_run(text: &str) -> Run {
    Run::new()
        .add_text(text)
        .fonts(body_fonts())
        .size(DOCX_BODY_FONT_SIZE)
}

fn create_styled_doc() -> Docx {
    let fonts = body_fonts();
    Docx::new()
        .add_style(
            Style::new("Normal", StyleType::Paragraph)
                .name("Normal")
                .fonts(fonts.clone())
                .size(DOCX_BODY_FONT_SIZE)
                .line_spacing(LineSpacing::new().line(320).after(160)),
        )
        .add_style(
            Style::new("Heading1", StyleType::Paragraph)
                .name("Heading 1")
                .fonts(fonts.clone())
                .bold()
                .size(40)
                .line_spacing(LineSpacing::new().before(320).after(220)),
        )
        .add_style(
            Style::new("Heading2", StyleType::Paragraph)
                .name("Heading 2")
                .fonts(fonts.clone())
                .bold()
                .size(32)
                .line_spacing(LineSpacing::new().before(260).after(180)),
        )
        .add_style(
            Style::new("Heading3", StyleType::Paragraph)
                .name("Heading 3")
                .fonts(fonts.clone())
                .bold()
                .size(28)
                .line_spacing(LineSpacing::new().before(220).after(140)),
        )
        .add_style(
            Style::new("Heading4", StyleType::Paragraph)
                .name("Heading 4")
                .fonts(fonts.clone())
                .bold()
                .size(24)
                .line_spacing(LineSpacing::new().before(180).after(120)),
        )
        .add_style(
            Style::new("Heading5", StyleType::Paragraph)
                .name("Heading 5")
                .fonts(fonts.clone())
                .bold()
                .size(22)
                .line_spacing(LineSpacing::new().before(160).after(100)),
        )
        .add_style(
            Style::new("Heading6", StyleType::Paragraph)
                .name("Heading 6")
                .fonts(fonts)
                .bold()
                .size(22)
                .line_spacing(LineSpacing::new().before(140).after(100)),
        )
}

fn convert_markdown_to_docx(markdown: &str) -> Result<Vec<u8>, AppError> {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS;

    let parser = Parser::new_ext(markdown, options);
    let events: Vec<Event> = parser.collect();

    let mut doc = create_styled_doc();

    let mut i = 0;
    while i < events.len() {
        i = process_event(&events, i, &mut doc)?;
    }

    let mut buf = Cursor::new(Vec::new());
    doc.build()
        .pack(&mut buf)
        .map_err(|e| AppError::InternalError(format!("DOCX 打包失败: {e}")))?;
    Ok(buf.into_inner())
}

fn process_event(events: &[Event], start: usize, doc: &mut Docx) -> Result<usize, AppError> {
    match &events[start] {
        Event::Start(Tag::Heading { level, .. }) => {
            let (runs, end) = collect_inline_runs(events, start + 1, TagEnd::Heading(*level));
            let style_name = match *level {
                pulldown_cmark::HeadingLevel::H1 => "Heading1",
                pulldown_cmark::HeadingLevel::H2 => "Heading2",
                pulldown_cmark::HeadingLevel::H3 => "Heading3",
                pulldown_cmark::HeadingLevel::H4 => "Heading4",
                pulldown_cmark::HeadingLevel::H5 => "Heading5",
                pulldown_cmark::HeadingLevel::H6 => "Heading6",
            };
            let mut para = Paragraph::new()
                .style(style_name)
                .line_spacing(LineSpacing::new().after(120));
            for run in runs {
                para = para.add_run(run);
            }
            *doc = std::mem::take(doc).add_paragraph(para);
            Ok(end + 1)
        }

        Event::Start(Tag::Paragraph) => {
            let (runs, end) = collect_inline_runs(events, start + 1, TagEnd::Paragraph);
            let mut para = Paragraph::new()
                .style("Normal")
                .line_spacing(LineSpacing::new().line(320).after(120));
            for run in runs {
                para = para.add_run(run);
            }
            *doc = std::mem::take(doc).add_paragraph(para);
            Ok(end + 1)
        }

        Event::Start(Tag::CodeBlock(kind)) => {
            let lang = match kind {
                CodeBlockKind::Fenced(info) => info.to_string(),
                CodeBlockKind::Indented => String::new(),
            };
            let mut code_text = String::new();
            let mut j = start + 1;
            while j < events.len() {
                match &events[j] {
                    Event::Text(t) => code_text.push_str(t),
                    Event::End(TagEnd::CodeBlock) => break,
                    _ => {}
                }
                j += 1;
            }
            let code_text = code_text.trim_end_matches('\n');

            if !lang.is_empty() {
                let lang_para = Paragraph::new().add_run(
                    body_run(&lang).size(18).color("888888").italic(),
                );
                *doc = std::mem::take(doc).add_paragraph(lang_para);
            }

            for line in code_text.split('\n') {
                let para = Paragraph::new()
                    .add_run(
                        Run::new()
                            .add_text(line)
                            .size(DOCX_CODE_FONT_SIZE)
                            .fonts(code_fonts()),
                    )
                    .indent(Some(280), None, None, None)
                    .line_spacing(LineSpacing::new().line(280).after(80));
                *doc = std::mem::take(doc).add_paragraph(para);
            }
            *doc = std::mem::take(doc).add_paragraph(
                Paragraph::new()
                    .style("Normal")
                    .line_spacing(LineSpacing::new().after(120)),
            );
            Ok(j + 1)
        }

        Event::Start(Tag::Table(alignments)) => {
            let aligns = alignments.clone();
            let (rows, end) = collect_table_rows(events, start + 1)?;
            let table = build_table(rows, &aligns);
            *doc = std::mem::take(doc).add_table(table);
            *doc = std::mem::take(doc).add_paragraph(Paragraph::new());
            Ok(end + 1)
        }

        Event::Start(Tag::List(start_num)) => {
            let ordered = start_num.is_some();
            let (items, end) = collect_list_items(events, start + 1)?;
            for (idx, item_runs) in items.into_iter().enumerate() {
                let bullet = if ordered {
                    format!("{}. ", idx + 1)
                } else {
                    "• ".to_string()
                };
                let mut para = Paragraph::new()
                    .style("Normal")
                    .line_spacing(LineSpacing::new().line(320).after(80))
                    .add_run(body_run(&bullet))
                    .indent(Some(360), None, None, None);
                for run in item_runs {
                    para = para.add_run(run);
                }
                *doc = std::mem::take(doc).add_paragraph(para);
            }
            Ok(end + 1)
        }

        Event::Start(Tag::BlockQuote(_)) => {
            let (runs, end) = collect_blockquote_runs(events, start + 1);
            let borders = ParagraphBorders::with_empty().set(
                ParagraphBorder::new(ParagraphBorderPosition::Left)
                    .size(12)
                    .color("CCCCCC"),
            );
            let mut para = Paragraph::new()
                .style("Normal")
                .line_spacing(LineSpacing::new().line(300).after(120))
                .indent(Some(400), None, None, None);
            para.property = para.property.set_borders(borders);
            for run in runs {
                para = para.add_run(run);
            }
            *doc = std::mem::take(doc).add_paragraph(para);
            Ok(end + 1)
        }

        Event::Text(t) => {
            let para = Paragraph::new()
                .style("Normal")
                .line_spacing(LineSpacing::new().line(320).after(120))
                .add_run(body_run(t.as_ref()));
            *doc = std::mem::take(doc).add_paragraph(para);
            Ok(start + 1)
        }

        Event::Rule => {
            let borders = ParagraphBorders::with_empty().set(
                ParagraphBorder::new(ParagraphBorderPosition::Bottom)
                    .size(6)
                    .color("999999"),
            );
            let mut para = Paragraph::new();
            para.property = para.property.set_borders(borders);
            *doc = std::mem::take(doc).add_paragraph(para);
            Ok(start + 1)
        }

        _ => Ok(start + 1),
    }
}

fn collect_inline_runs(events: &[Event], start: usize, end_tag: TagEnd) -> (Vec<Run>, usize) {
    let mut runs: Vec<Run> = Vec::new();
    let mut bold = false;
    let mut italic = false;
    let mut strikethrough = false;
    let mut j = start;

    while j < events.len() {
        match &events[j] {
            Event::End(tag) if *tag == end_tag => return (runs, j),
            Event::Start(Tag::Strong) => bold = true,
            Event::End(TagEnd::Strong) => bold = false,
            Event::Start(Tag::Emphasis) => italic = true,
            Event::End(TagEnd::Emphasis) => italic = false,
            Event::Start(Tag::Strikethrough) => strikethrough = true,
            Event::End(TagEnd::Strikethrough) => strikethrough = false,
            Event::Start(Tag::Link { dest_url, .. }) => {
                let mut link_text = String::new();
                let mut k = j + 1;
                while k < events.len() {
                    match &events[k] {
                        Event::Text(t) => link_text.push_str(t),
                        Event::End(TagEnd::Link) => break,
                        _ => {}
                    }
                    k += 1;
                }
                let display = if link_text.is_empty() {
                    dest_url.to_string()
                } else {
                    link_text
                };
                let mut run = body_run(&display).color("2563EB").underline("single");
                if bold {
                    run = run.bold();
                }
                if italic {
                    run = run.italic();
                }
                runs.push(run);
                j = k;
            }
            Event::Start(Tag::Image { dest_url, .. }) => {
                let mut alt_text = String::new();
                let mut k = j + 1;
                while k < events.len() {
                    match &events[k] {
                        Event::Text(t) => alt_text.push_str(t),
                        Event::Code(code) => alt_text.push_str(code),
                        Event::End(TagEnd::Image) => break,
                        _ => {}
                    }
                    k += 1;
                }

                if let Some(image_run) = build_image_run(dest_url.as_ref(), &alt_text) {
                    runs.push(image_run);
                } else {
                    let fallback = if alt_text.trim().is_empty() {
                        format!("[图片未导出: {}]", dest_url)
                    } else {
                        format!("[图片未导出: {} ({})]", alt_text.trim(), dest_url)
                    };
                    runs.push(body_run(&fallback).italic().color("999999"));
                }
                j = k;
            }
            Event::Text(t) => {
                let mut run = body_run(t.as_ref());
                if bold {
                    run = run.bold();
                }
                if italic {
                    run = run.italic();
                }
                if strikethrough {
                    run = run.strike();
                }
                runs.push(run);
            }
            Event::Code(code) => {
                let mut run = Run::new()
                    .add_text(code.as_ref())
                    .fonts(code_fonts())
                    .size(DOCX_CODE_FONT_SIZE)
                    .color("C7254E");
                if bold {
                    run = run.bold();
                }
                runs.push(run);
            }
            Event::SoftBreak | Event::HardBreak => {
                runs.push(Run::new().add_break(BreakType::TextWrapping));
            }
            _ => {}
        }
        j += 1;
    }
    (runs, j)
}

fn build_image_run(image_src: &str, alt_text: &str) -> Option<Run> {
    let image_bytes = load_markdown_image_bytes(image_src)?;
    let dynamic_image = image::load_from_memory(&image_bytes).ok()?;
    let (width_px, height_px) = dynamic_image.dimensions();

    let mut png_bytes = Cursor::new(Vec::new());
    dynamic_image
        .write_to(&mut png_bytes, image::ImageFormat::Png)
        .ok()?;

    let (target_width_px, target_height_px) =
        fit_image_size(width_px.max(1), height_px.max(1), DOCX_IMAGE_MAX_WIDTH_PX);
    let pic =
        Pic::new_with_dimensions(png_bytes.into_inner(), target_width_px, target_height_px).size(
            target_width_px.saturating_mul(DOCX_EMU_PER_PX),
            target_height_px.saturating_mul(DOCX_EMU_PER_PX),
        );

    let mut run = Run::new().add_image(pic);
    if !alt_text.trim().is_empty() {
        run = run
            .add_break(BreakType::TextWrapping)
            .add_text(alt_text.trim())
            .fonts(body_fonts())
            .size(18)
            .color("666666")
            .italic();
    }
    Some(run)
}

fn fit_image_size(width_px: u32, height_px: u32, max_width_px: u32) -> (u32, u32) {
    if width_px <= max_width_px {
        return (width_px, height_px);
    }
    let ratio = max_width_px as f64 / width_px as f64;
    let target_height = ((height_px as f64) * ratio).round() as u32;
    (max_width_px, target_height.max(1))
}

fn load_markdown_image_bytes(image_src: &str) -> Option<Vec<u8>> {
    let trimmed = image_src.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(bytes) = decode_data_uri_image(trimmed) {
        return Some(bytes);
    }

    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return fetch_remote_image(trimmed);
    }

    let decoded = urlencoding::decode(trimmed).ok()?.into_owned();
    let path = normalize_image_path(&decoded);
    std::fs::read(path).ok()
}

fn decode_data_uri_image(image_src: &str) -> Option<Vec<u8>> {
    if !image_src.starts_with("data:image/") {
        return None;
    }
    let (meta, data) = image_src.split_once(',')?;
    if !meta.contains(";base64") {
        return None;
    }
    STANDARD.decode(data).ok()
}

fn fetch_remote_image(image_src: &str) -> Option<Vec<u8>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .ok()?;
    let response = client.get(image_src).send().ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.bytes().ok().map(|bytes| bytes.to_vec())
}

fn normalize_image_path(path: &str) -> PathBuf {
    let mut normalized = path.to_string();
    if let Some(rest) = normalized.strip_prefix("file:///") {
        normalized = rest.to_string();
    } else if let Some(rest) = normalized.strip_prefix("file://") {
        normalized = rest.to_string();
    }

    if cfg!(windows) {
        normalized = normalized.replace('/', "\\");
        if normalized.starts_with('\\') && normalized.chars().nth(2) == Some(':') {
            normalized = normalized.trim_start_matches('\\').to_string();
        }
    }

    PathBuf::from(normalized)
}

fn collect_blockquote_runs(events: &[Event], start: usize) -> (Vec<Run>, usize) {
    let mut runs: Vec<Run> = Vec::new();
    let mut j = start;
    let mut depth = 0;

    while j < events.len() {
        match &events[j] {
            Event::Start(Tag::BlockQuote(_)) => depth += 1,
            Event::End(TagEnd::BlockQuote(_)) => {
                if depth == 0 {
                    return (runs, j);
                }
                depth -= 1;
            }
            Event::Start(Tag::Paragraph) => {}
            Event::End(TagEnd::Paragraph) => {
                runs.push(Run::new().add_break(BreakType::TextWrapping));
            }
            Event::Text(t) => {
                runs.push(body_run(t.as_ref()).color("666666").italic());
            }
            Event::Code(code) => {
                runs.push(
                    Run::new()
                        .add_text(code.as_ref())
                        .fonts(code_fonts())
                        .size(DOCX_CODE_FONT_SIZE)
                        .color("C7254E"),
                );
            }
            Event::SoftBreak | Event::HardBreak => {
                runs.push(Run::new().add_break(BreakType::TextWrapping));
            }
            _ => {}
        }
        j += 1;
    }
    (runs, j)
}

fn collect_table_rows(
    events: &[Event],
    start: usize,
) -> Result<(Vec<Vec<Vec<Run>>>, usize), AppError> {
    let mut rows: Vec<Vec<Vec<Run>>> = Vec::new();
    let mut current_row: Vec<Vec<Run>> = Vec::new();
    let mut current_cell_runs: Vec<Run> = Vec::new();
    let mut in_cell = false;
    let mut bold_ctx = false;
    let mut j = start;

    while j < events.len() {
        match &events[j] {
            Event::End(TagEnd::Table) => {
                return Ok((rows, j));
            }
            Event::Start(Tag::TableHead) | Event::Start(Tag::TableRow) => {
                current_row = Vec::new();
            }
            Event::End(TagEnd::TableHead) | Event::End(TagEnd::TableRow) => {
                rows.push(std::mem::take(&mut current_row));
            }
            Event::Start(Tag::TableCell) => {
                in_cell = true;
                current_cell_runs = Vec::new();
                if rows.is_empty() {
                    bold_ctx = true;
                }
            }
            Event::End(TagEnd::TableCell) => {
                in_cell = false;
                bold_ctx = false;
                current_row.push(std::mem::take(&mut current_cell_runs));
            }
            Event::Text(t) if in_cell => {
                let mut run = body_run(t.as_ref());
                if bold_ctx {
                    run = run.bold();
                }
                current_cell_runs.push(run);
            }
            Event::Code(code) if in_cell => {
                current_cell_runs.push(
                    Run::new()
                        .add_text(code.as_ref())
                        .fonts(code_fonts())
                        .size(DOCX_CODE_FONT_SIZE)
                        .color("C7254E"),
                );
            }
            Event::Start(Tag::Strong) if in_cell => bold_ctx = true,
            Event::End(TagEnd::Strong) if in_cell => {
                if !rows.is_empty() {
                    bold_ctx = false;
                }
            }
            _ => {}
        }
        j += 1;
    }
    Ok((rows, j))
}

fn build_table(rows: Vec<Vec<Vec<Run>>>, _aligns: &[pulldown_cmark::Alignment]) -> Table {
    let mut table_rows: Vec<TableRow> = Vec::new();

    for (row_idx, row) in rows.into_iter().enumerate() {
        let mut cells: Vec<TableCell> = Vec::new();
        for cell_runs in row {
            let mut para = Paragraph::new();
            for run in cell_runs {
                para = para.add_run(run);
            }
            let mut cell = TableCell::new().add_paragraph(para);
            if row_idx == 0 {
                cell = cell.shading(Shading::new().shd_type(ShdType::Clear).fill("F0F0F0"));
            }
            cells.push(cell);
        }
        table_rows.push(TableRow::new(cells));
    }

    Table::new(table_rows)
}

fn collect_list_items(events: &[Event], start: usize) -> Result<(Vec<Vec<Run>>, usize), AppError> {
    let mut items: Vec<Vec<Run>> = Vec::new();
    let mut j = start;

    while j < events.len() {
        match &events[j] {
            Event::End(TagEnd::List(_)) => {
                return Ok((items, j));
            }
            Event::Start(Tag::Item) => {
                let (runs, end) = collect_inline_runs(events, j + 1, TagEnd::Item);
                items.push(runs);
                j = end;
            }
            _ => {}
        }
        j += 1;
    }
    Ok((items, j))
}
