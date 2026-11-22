function projectPath(files: Record<string, string>, ref: string, from: string) {
    if (!ref || /^(?:[\w+-]+:|\/\/|#)/.test(ref)) return;
    let path: string;
    try {
        path = decodeURIComponent(ref.split(/[?#]/, 1)[0]);
    } catch {
        return;
    }
    const parts = path.startsWith("/") ? [] : from.split("/").slice(0, -1);
    for (const part of path.split("/")) {
        if (part === "..") {
            if (!parts.length) return;
            parts.pop();
        } else if (part && part !== ".") parts.push(part);
    }
    const name = parts.join("/");
    return [name, "public/" + name].find((key) => Object.hasOwn(files, key));
}

const types: Record<string, string> = {
    svg: "image/svg+xml",
    png: "image/png",
    jpeg: "image/jpeg",
    jpg: "image/jpeg",
    webp: "image/webp",
    gif: "image/gif",
    avif: "image/avif",
    ico: "image/x-icon",
    woff2: "font/woff2",
    woff: "font/woff",
    mp4: "video/mp4",
    mp3: "audio/mpeg",
};

function asset(files: Record<string, string>, name: string) {
    const mime = types[name.split(".").pop()?.toLowerCase() || ""] || "application/octet-stream";
    return "data:" + mime + ";base64," + files[name];
}

export function projectImage(files: Record<string, string>, entry: string, src: string) {
    if (/^(?:https?:|\/\/)/i.test(src)) return src;
    const name = projectPath(files, src, entry);
    if (name && types[name.split(".").pop()?.toLowerCase() || ""]?.startsWith("image/")) {
        return asset(files, name);
    }
}

// Assemble ordinary HTML from this version's files before applying the shared preview resource policy.
export function projectHtml(files: Record<string, string>, entry: string): string | undefined {
    const template = document.createElement("template");
    const read = (name: string) =>
        new TextDecoder().decode(Uint8Array.from(atob(files[name]), (ch) => ch.charCodeAt(0)));
    template.innerHTML = read(entry);
    const root = template.content;
    // Module entry points use the isolated project compiler and its relative import resolver.
    if (root.querySelector('script[type="module"], script[src$=".tsx"], script[src$=".ts"], script[src$=".jsx"]'))
        return;
    const css = (css: string, from: string) =>
        css.replace(/url\(\s*(['"]?)([^)'"]+)\1\s*\)/g, (match, _, url: string) => {
            const name = projectPath(files, url.trim(), from);
            return name ? "url(" + asset(files, name) + ")" : match;
        });
    for (const tag of root.querySelectorAll(
        "script[src], link[rel=stylesheet][href], img[src], source[src], video[src], audio[src]"
    )) {
        const attr = tag.hasAttribute("href") ? "href" : "src";
        const name = projectPath(files, tag.getAttribute(attr)!, entry);
        if (!name) continue;
        if (tag.tagName === "LINK") {
            const style = document.createElement("style");
            style.textContent = css(read(name), name).replace(/<\/style/gi, "<\\/style");
            tag.replaceWith(style);
        } else if (tag.tagName === "SCRIPT") {
            tag.removeAttribute(attr);
            tag.textContent = read(name).replace(/<\/script/gi, "<\\/script");
        } else tag.setAttribute(attr, asset(files, name));
    }
    for (const style of root.querySelectorAll("style")) style.textContent = css(style.textContent || "", entry);
    return "<!doctype html>" + template.innerHTML;
}
