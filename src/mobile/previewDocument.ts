export const STATIC_PREVIEW_TYPES = new Set(["html", "svg", "xml", "markdown", "md", "text", "txt", "mermaid", "image"]);
export function staticPreviewDocument(markup: string): string {
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
    return `<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; img-src data:; form-action 'none'; base-uri 'none'"></head><body>${template.innerHTML}</body></html>`;
}
