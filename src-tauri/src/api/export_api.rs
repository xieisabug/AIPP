use crate::errors::AppError;
use docx_rs::*;
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use std::io::Cursor;

/// Markdown 转 Word (.docx) 字节流
#[tauri::command]
pub async fn markdown_to_docx(markdown: String) -> Result<Vec<u8>, AppError> {
    tokio::task::spawn_blocking(move || convert_markdown_to_docx(&markdown))
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?
}

fn convert_markdown_to_docx(markdown: &str) -> Result<Vec<u8>, AppError> {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS;

    let parser = Parser::new_ext(markdown, options);
    let events: Vec<Event> = parser.collect();

    let mut doc = Docx::new();

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
            let mut para = Paragraph::new().style(style_name);
            for run in runs {
                para = para.add_run(run);
            }
            *doc = std::mem::take(doc).add_paragraph(para);
            Ok(end + 1)
        }

        Event::Start(Tag::Paragraph) => {
            let (runs, end) = collect_inline_runs(events, start + 1, TagEnd::Paragraph);
            let mut para = Paragraph::new();
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
                    Run::new()
                        .add_text(&lang)
                        .size(16)
                        .color("888888")
                        .italic(),
                );
                *doc = std::mem::take(doc).add_paragraph(lang_para);
            }

            for line in code_text.split('\n') {
                let para = Paragraph::new()
                    .add_run(
                        Run::new()
                            .add_text(line)
                            .size(18)
                            .fonts(RunFonts::new().ascii("Consolas").hi_ansi("Consolas")),
                    )
                    .indent(Some(280), None, None, None);
                *doc = std::mem::take(doc).add_paragraph(para);
            }
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
                    .add_run(Run::new().add_text(&bullet))
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
            let mut para = Paragraph::new().indent(Some(400), None, None, None);
            para.property = para.property.set_borders(borders);
            for run in runs {
                para = para.add_run(run);
            }
            *doc = std::mem::take(doc).add_paragraph(para);
            Ok(end + 1)
        }

        Event::Text(t) => {
            let para = Paragraph::new().add_run(Run::new().add_text(t.as_ref()));
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
                let mut run = Run::new().add_text(&display).color("2563EB");
                if bold {
                    run = run.bold();
                }
                if italic {
                    run = run.italic();
                }
                runs.push(run);
                j = k;
            }
            Event::Text(t) => {
                let mut run = Run::new().add_text(t.as_ref());
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
                    .fonts(RunFonts::new().ascii("Consolas").hi_ansi("Consolas"))
                    .size(19)
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
                runs.push(Run::new().add_text(t.as_ref()).color("666666").italic());
            }
            Event::Code(code) => {
                runs.push(
                    Run::new()
                        .add_text(code.as_ref())
                        .fonts(RunFonts::new().ascii("Consolas").hi_ansi("Consolas"))
                        .size(19)
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
                let mut run = Run::new().add_text(t.as_ref());
                if bold_ctx {
                    run = run.bold();
                }
                current_cell_runs.push(run);
            }
            Event::Code(code) if in_cell => {
                current_cell_runs.push(
                    Run::new()
                        .add_text(code.as_ref())
                        .fonts(RunFonts::new().ascii("Consolas").hi_ansi("Consolas"))
                        .size(19)
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
