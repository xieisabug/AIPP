//! 本轮附加上下文：系统随用户消息附带给模型的当前时间、工作记录、引用的工作、长期记忆、外部渠道来源等。
//!
//! 这块内容用 `<aipp_turn_context>` 包住，放在用户消息正文最末尾（附件之后），和用户原话一起存进数据库。
//! 模型历史因此逐字回放当时实际发送的内容：缓存前缀保持稳定，工具续写、重新生成也不会丢掉这一轮的上下文，
//! 模型也能看到自己之前收到过什么。
//! 给人看或检索的地方（气泡、摘要、搜索、标题、外部渠道转发）只看用户原话，读取时用 [`visible`] 或 [`visible_sql`] 去掉。
//! 识别规则：正文以 `</aipp_turn_context>` 结尾，且含有 [`wrap`] 写出的块开头（开标签紧跟说明行），从第一处这样的开头算起。
//! 只认开标签的话，用户自己打出 `<aipp_turn_context>…</aipp_turn_context>` 会把后面的原话当成附加上下文藏掉；
//! 说明行只由系统写出，用户不会恰好打出这一整行。取第一处（而不是最后一处）是因为块里的工作记录、引用内容
//! 可能转述带附加上下文的原文，SQL 也只能方便地找第一处，三端规则保持一致。
//!
//! 与 `src/utils/turnContext.ts` 保持一致。

pub const CLOSE: &str = "</aipp_turn_context>";
/// 块开头：开标签 + 换行 + 说明行（告诉模型这不是用户写的内容）。只由 [`wrap`] 写出，识别附加上下文以它为准。
const START: &str = "<aipp_turn_context>\n以下是系统随这条消息附带的上下文，不是用户写的内容。";

/// 把若干段上下文包成一块；全部为空时返回 None。
pub fn wrap<I, S>(sections: I) -> Option<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let body: Vec<String> = sections
        .into_iter()
        .map(|section| section.as_ref().trim().to_string())
        .filter(|section| !section.is_empty())
        .collect();
    if body.is_empty() {
        return None;
    }
    Some(format!("{START}\n{}\n{CLOSE}", body.join("\n\n")))
}

/// 把附加上下文接在消息正文末尾。
pub fn append(content: &str, context: Option<&str>) -> String {
    match context.map(str::trim).filter(|context| !context.is_empty()) {
        None => content.to_string(),
        Some(context) if content.trim().is_empty() => context.to_string(),
        Some(context) => format!("{}\n\n{context}", content.trim_end()),
    }
}

/// 拆成（给人看的部分，附加上下文的正文）。没有附加上下文时原样返回。
pub fn split(content: &str) -> (&str, Option<&str>) {
    let trimmed = content.trim_end();
    let Some((at, body)) = trimmed.strip_suffix(CLOSE).and_then(|body| Some((body.find(START)?, body))) else {
        return (content, None);
    };
    (trimmed[..at].trim_end(), Some(body[at + START.len()..].trim()))
}

/// 用户消息给人看的部分。
pub fn visible(content: &str) -> &str {
    split(content).0
}

/// 附加上下文的正文（不含外层标签和说明行）。
pub fn context(content: &str) -> Option<&str> {
    split(content).1
}

/// SQL 表达式：某列给人看的部分，规则与 [`visible`] 相同，供检索和摘要在数据库里直接使用。
pub fn visible_sql(column: &str) -> String {
    // 块开头里的换行在 SQL 字符串字面量里原样保留，instr 按字符计位置，与 substr 一致。
    format!(
        "(CASE WHEN instr({column},'{START}')>0 AND substr(rtrim({column},char(9,10,13,32)),-{close_len})='{CLOSE}' \
         THEN rtrim(substr({column},1,instr({column},'{START}')-1),char(9,10,13,32)) ELSE {column} END)",
        close_len = CLOSE.chars().count(),
    )
}

/// `text` 里所有 `<tag …>…</tag>` 元素，按出现顺序返回（含首尾标签）。
pub fn elements<'a>(text: &'a str, tag: &str) -> Vec<&'a str> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(offset) = text[from..].find(&open) {
        let start = from + offset;
        let after = &text[start + open.len()..];
        // `<tag>` 或 `<tag 属性…>`；`<tagx>` 这类更长的标签名不算。
        if !(after.starts_with('>') || after.starts_with(char::is_whitespace)) {
            from = start + open.len();
            continue;
        }
        let Some(end) = text[start..].find(&close) else { break };
        let end = start + end + close.len();
        found.push(&text[start..end]);
        from = end;
    }
    found
}

/// 在按时间排列的多段附加上下文里，找最后出现的某种元素（多个标签名时取最靠后的那一个）。
pub fn last_element<'a>(contexts: &'a [String], tags: &[&str]) -> Option<&'a str> {
    contexts.iter().rev().find_map(|context| {
        tags.iter()
            .filter_map(|tag| elements(context, tag).last().copied())
            .max_by_key(|element| element.as_ptr() as usize)
    })
}

/// 元素开始标签里的属性值，例如 `<long_term_memory as_of="1">` 的 `as_of`。
pub fn attribute<'a>(element: &'a str, name: &str) -> Option<&'a str> {
    let head = &element[..element.find('>')?];
    let marker = format!(" {name}=\"");
    let start = head.find(&marker)? + marker.len();
    let end = head[start..].find('"')? + start;
    Some(&head[start..end])
}
