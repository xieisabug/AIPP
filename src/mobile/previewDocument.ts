export const STATIC_PREVIEW_TYPES = new Set(["html", "svg", "xml", "markdown", "md", "text", "txt", "mermaid", "image"]);
/** 一次性随机值，用于只放行宿主自己追加的那段脚本。 */
export function randomNonce(): string {
    return Array.from(crypto.getRandomValues(new Uint8Array(16)), (b) => b.toString(16).padStart(2, "0")).join("");
}
/**
 * `hostScript` 为宿主追加的可信脚本（如比选预览的测高脚本）。CSP 只放行带本次 nonce 的这一段，
 * 内容里的脚本仍在上面被移除；调用方需相应给 iframe 加 `sandbox="allow-scripts"`（不加 allow-same-origin）。
 */
export function staticPreviewDocument(markup: string, hostScript?: string): string {
    // Template contents stay inert and detached while untrusted markup is parsed.
    const template = document.createElement("template");
    template.innerHTML = markup;
    const parsed = template.content;
    parsed.querySelectorAll("script,iframe,frame,object,embed,base,meta,link,form").forEach(node => node.remove());
    parsed.querySelectorAll("*").forEach(node => {
        for (const attribute of Array.from(node.attributes)) {
            if (/^on/i.test(attribute.name) || /^(href|xlink:href|action|formaction|srcdoc|target)$/i.test(attribute.name)) node.removeAttribute(attribute.name);
        }
    });
    const nonce = hostScript ? randomNonce() : "";
    const scriptSrc = hostScript ? ` script-src 'nonce-${nonce}';` : "";
    const tail = hostScript ? `<script nonce="${nonce}">${hostScript}</script>` : "";
    return `<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><meta http-equiv="Content-Security-Policy" content="default-src 'none';${scriptSrc} style-src 'unsafe-inline'; img-src data:; form-action 'none'; base-uri 'none'"></head><body>${template.innerHTML}${tail}</body></html>`;
}
