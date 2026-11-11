/** 用户消息里渲染成卡片的自定义标签（附件、技能、网页、提示），RawTextRenderer 与折叠共用这条规则。 */
export const CUSTOM_TAG_PATTERN =
    /<(fileattachment|skillattachment|bangwebtomarkdown|bangweb|tipscomponent)\b[^>]*>[\s\S]*?<\/\1>|<(fileattachment|skillattachment|bangwebtomarkdown|bangweb|tipscomponent)\b[^>]*\/>/gi;

/**
 * 长用户消息折叠：只按显示出来的文字计数和截断，附件等标签整块保留（它们显示成卡片），
 * 避免截断把标签切开、露出原文。不需要折叠时返回 null。
 */
export function collapseUserMessage(content: string, limit: number): string | null {
    const segments: Array<{ tag: boolean; text: string }> = [];
    let cursor = 0;
    for (const match of content.matchAll(CUSTOM_TAG_PATTERN)) {
        const start = match.index ?? 0;
        if (start > cursor) segments.push({ tag: false, text: content.slice(cursor, start) });
        segments.push({ tag: true, text: match[0] });
        cursor = start + match[0].length;
    }
    if (cursor < content.length) segments.push({ tag: false, text: content.slice(cursor) });

    const visible = segments.reduce((sum, segment) => segment.tag ? sum : sum + Array.from(segment.text).length, 0);
    if (visible <= limit) return null;

    let remaining = limit;
    let truncated = false;
    let result = "";
    for (const segment of segments) {
        if (segment.tag) {
            result += segment.text;
            continue;
        }
        if (truncated) continue;
        const characters = Array.from(segment.text);
        if (characters.length <= remaining) {
            result += segment.text;
            remaining -= characters.length;
            continue;
        }
        result += `${characters.slice(0, remaining).join("")}…`;
        truncated = true;
    }
    return result;
}
