//! Read the text the ordinary chat API can consume. Native agents receive the original path.
use anyhow::{anyhow, bail, ensure, Context, Result};
use std::{collections::HashMap, io::{Read, Seek}};
use xml::reader::{EventReader, XmlEvent};
use zip::ZipArchive;

const MAX_XML_BYTES: usize = 16 * 1024 * 1024;
const MAX_TEXT_BYTES: usize = 128 * 1024;

struct Parts<R: Read + Seek> {
    archive: ZipArchive<R>,
    remaining: usize,
}

impl<R: Read + Seek> Parts<R> {
    fn visit(&mut self, path: &str, mut visit: impl FnMut(XmlEvent) -> Result<()>) -> Result<()> {
        let entry = self.archive.by_name(path).with_context(|| format!("缺少 {path}"))?;
        ensure!(entry.size() <= self.remaining as u64, "文档解压内容过大，请选择所需区域或使用本地 Agent");
        let mut bytes = Vec::new();
        entry.take(self.remaining as u64 + 1).read_to_end(&mut bytes)?;
        ensure!(bytes.len() <= self.remaining, "文档解压内容超限");
        self.remaining -= bytes.len();
        for event in EventReader::new(bytes.as_slice()) {
            visit(event.with_context(|| format!("解析 {path} 失败"))?)?;
        }
        Ok(())
    }
}

fn append(out: &mut String, text: &str) -> Result<()> {
    ensure!(out.len() + text.len() <= MAX_TEXT_BYTES, "文档文字超过 128 KiB，请选择所需区域或使用本地 Agent");
    out.push_str(text);
    Ok(())
}

fn paragraphs<R: Read + Seek>(parts: &mut Parts<R>, path: &str, out: &mut String) -> Result<()> {
    let mut in_text = false;
    parts.visit(path, |event| {
        match event {
            XmlEvent::StartElement { name, .. } if name.local_name == "t" => in_text = true,
            XmlEvent::EndElement { name } if name.local_name == "t" => in_text = false,
            XmlEvent::EndElement { name } if name.local_name == "p" => append(out, "\n")?,
            XmlEvent::Characters(s) | XmlEvent::Whitespace(s) | XmlEvent::CData(s) if in_text => append(out, &s)?,
            _ => {}
        }
        Ok(())
    })
}

fn attr<'a>(attrs: &'a [xml::attribute::OwnedAttribute], key: &str) -> Option<&'a str> {
    attrs.iter().find(|a| a.name.local_name == key).map(|a| a.value.as_str())
}

fn spreadsheet<R: Read + Seek>(parts: &mut Parts<R>, out: &mut String) -> Result<()> {
    let mut strings = Vec::new();
    if parts.archive.index_for_name("xl/sharedStrings.xml").is_some() {
        let mut current = String::new();
        let mut in_text = false;
        parts.visit("xl/sharedStrings.xml", |event| {
            match event {
                XmlEvent::StartElement { name, .. } if name.local_name == "t" => in_text = true,
                XmlEvent::EndElement { name } if name.local_name == "t" => in_text = false,
                XmlEvent::EndElement { name } if name.local_name == "si" => strings.push(std::mem::take(&mut current)),
                XmlEvent::Characters(s) | XmlEvent::Whitespace(s) if in_text => current.push_str(&s),
                _ => {}
            }
            Ok(())
        })?;
    }
    let mut targets = HashMap::new();
    parts.visit("xl/_rels/workbook.xml.rels", |event| {
        if let XmlEvent::StartElement { name, attributes, .. } = event {
            if name.local_name == "Relationship" {
                if let (Some(id), Some(target)) = (attr(&attributes, "Id"), attr(&attributes, "Target")) {
                    let path = if target.starts_with('/') { target[1..].to_owned() } else { format!("xl/{target}") };
                    targets.insert(id.to_owned(), path);
                }
            }
        }
        Ok(())
    })?;
    let mut sheets = Vec::new();
    parts.visit("xl/workbook.xml", |event| {
        if let XmlEvent::StartElement { name, attributes, .. } = event {
            if name.local_name == "sheet" {
                let title = attr(&attributes, "name").context("工作表缺少名称")?;
                let id = attr(&attributes, "id").context("工作表缺少关系标识")?;
                sheets.push((title.to_owned(), targets.get(id).context("找不到工作表内容")?.clone()));
            }
        }
        Ok(())
    })?;
    for (title, path) in sheets {
        append(out, &format!("[{title}]\n"))?;
        let (mut cell, mut kind, mut value) = (String::new(), String::new(), String::new());
        let mut in_value = false;
        parts.visit(&path, |event| {
            match event {
                XmlEvent::StartElement { name, attributes, .. } if name.local_name == "c" => {
                    cell = attr(&attributes, "r").unwrap_or("").to_owned();
                    kind = attr(&attributes, "t").unwrap_or("").to_owned();
                    value.clear();
                }
                XmlEvent::StartElement { name, .. } if matches!(name.local_name.as_str(), "v" | "t") => in_value = true,
                XmlEvent::EndElement { name } if matches!(name.local_name.as_str(), "v" | "t") => in_value = false,
                XmlEvent::Characters(s) | XmlEvent::Whitespace(s) if in_value => value.push_str(&s),
                XmlEvent::EndElement { name } if name.local_name == "c" && !value.is_empty() => {
                    let text = if kind == "s" {
                        strings.get(value.parse::<usize>()?).ok_or_else(|| anyhow!("无效的共享字符串编号 {value}"))?
                    } else { &value };
                    append(out, &format!("{cell}\t{text}\n"))?;
                }
                _ => {}
            }
            Ok(())
        })?;
    }
    Ok(())
}

pub(crate) fn extract(reader: impl Read + Seek, kind: crate::db::conversation_db::AttachmentType) -> Result<String> {
    use crate::db::conversation_db::AttachmentType;
    let mut parts = Parts { archive: ZipArchive::new(reader)?, remaining: MAX_XML_BYTES };
    let mut out = String::new();
    match kind {
        AttachmentType::Word => paragraphs(&mut parts, "word/document.xml", &mut out)?,
        AttachmentType::Excel => spreadsheet(&mut parts, &mut out)?,
        AttachmentType::PowerPoint => {
            let mut slides: Vec<u32> = parts.archive.file_names().filter_map(|p|
                p.strip_prefix("ppt/slides/slide")?.strip_suffix(".xml")?.parse().ok()).collect();
            slides.sort_unstable();
            for slide in slides {
                append(&mut out, &format!("[slide{slide}.xml]\n"))?;
                paragraphs(&mut parts, &format!("ppt/slides/slide{slide}.xml"), &mut out)?;
            }
        }
        _ => bail!("不支持的 Office 文件类型"),
    }
    ensure!(!out.trim().is_empty(), "文档没有可提取的文字，请发送选区、截图或使用本地 Agent");
    Ok(out)
}
