// Standalone worker-format bundle loaded as a classic script in an opaque-origin frame.
import * as React from "react";
import * as ReactDOM from "react-dom/client";
import * as Vue from "vue/dist/vue.esm-bundler.js";
import { parse, compileScript } from "vue/compiler-sfc";
import { transform } from "@babel/standalone";

export function mountProject(files: Record<string, string>, entry: string) {
    const modules = new Map<string, { exports: any }>();
    const decoder = new TextDecoder();
    const read = (name: string) => decoder.decode(Uint8Array.from(atob(files[name]), (c) => c.charCodeAt(0)));
    const mime: Record<string, string> = {
        svg: "image/svg+xml",
        png: "image/png",
        jpg: "image/jpeg",
        jpeg: "image/jpeg",
        gif: "image/gif",
        webp: "image/webp",
        ico: "image/x-icon",
        woff2: "font/woff2",
        woff: "font/woff",
    };
    function resolve(from: string, spec: string) {
        const path = spec.startsWith("/") ? [] : from.split("/").slice(0, -1);
        for (const part of spec.split("/")) {
            if (part === "..") {
                if (!path.length) throw Error("引用超出工程：" + spec);
                path.pop();
            } else if (part && part !== ".") path.push(part);
        }
        const name = path.join("/");
        const candidates = [
            name,
            ...[".ts", ".tsx", ".js", ".jsx", ".vue", ".json", "/index.js", "/index.ts", "/index.tsx"].map(
                (ext) => name + ext
            ),
            "public/" + name,
        ];
        const found = candidates.find((p) => Object.hasOwn(files, p));
        if (!found) throw Error("工程缺少文件：" + from + " → " + spec);
        return found;
    }
    function asset(name: string) {
        return (
            "data:" + (mime[name.split(".").pop() || ""] || "application/octet-stream") + ";base64," + files[name]
        );
    }
    function css(name: string, text: string) {
        const style = document.createElement("style");
        style.textContent = text.replace(/url\(\s*['"]?([^'")\s]+)['"]?\s*\)/g, (original, url: string) => {
            if (/^(?:[\w+-]+:|#|\/\/)/.test(url)) return original;
            return "url(" + asset(resolve(name, url)) + ")";
        });
        document.head.append(style);
    }
    function compile(name: string, code: string, mod: { exports: any }) {
        const result = transform(code, {
            filename: name.replace(/\.vue$/, ".ts"),
            presets: ["typescript", ["react", { runtime: "classic" }]],
            plugins: ["transform-modules-commonjs"],
        });
        const require = (spec: string) => {
            if (spec === "react") return React;
            if (spec === "react-dom/client") return ReactDOM;
            if (spec === "vue") return Vue;
            if (spec.startsWith(".") || spec.startsWith("/")) return load(resolve(name, spec));
            throw Error("此工程依赖 " + spec + "，请在桌面端用 Bun 安装依赖并构建。");
        };
        new Function("module", "exports", "require", "React", result.code || "")(mod, mod.exports, require, React);
    }
    function load(name: string): any {
        const cached = modules.get(name);
        if (cached) return cached.exports;
        const mod = { exports: {} as any };
        modules.set(name, mod);
        if (/\.css$/i.test(name)) css(name, read(name));
        else if (/\.json$/i.test(name)) mod.exports = JSON.parse(read(name));
        else if (/\.vue$/i.test(name)) {
            const { descriptor, errors } = parse(read(name), { filename: name });
            if (errors.length) throw errors[0];
            if (descriptor.script || descriptor.scriptSetup) {
                const script = compileScript(descriptor, { id: name, inlineTemplate: !!descriptor.scriptSetup });
                compile(name, script.content, mod);
            } else {
                // Babel's default import helper expects template-only SFCs to be ES modules too.
                mod.exports = { __esModule: true, default: {} };
            }
            if (descriptor.template && !descriptor.scriptSetup)
                mod.exports.default.template = descriptor.template.content;
            for (const style of descriptor.styles)
                css(name, style.src ? read(resolve(name, style.src)) : style.content);
        } else if (/\.[cm]?[jt]sx?$/i.test(name)) compile(name, read(name), mod);
        else mod.exports = asset(name);
        return mod.exports;
    }
    function html() {
        const tpl = document.createElement("template");
        tpl.innerHTML = read(entry);
        const scripts = Array.from(tpl.content.querySelectorAll("script"));
        scripts.forEach((s) => s.remove());
        tpl.content.querySelectorAll("meta,base,iframe,object,embed").forEach((n) => n.remove());
        tpl.content.querySelectorAll<HTMLLinkElement>('link[rel="stylesheet"]').forEach((n) => {
            const p = resolve(entry, n.getAttribute("href")!);
            css(p, read(p));
            n.remove();
        });
        tpl.content.querySelectorAll<HTMLImageElement>("img[src]").forEach((n) => {
            const path = n.getAttribute("src")!;
            if (!path.startsWith("data:")) n.src = asset(resolve(entry, path));
        });
        document.getElementById("root")!.replaceWith(tpl.content);
        for (const script of scripts) {
            const src = script.getAttribute("src");
            if (src) load(resolve(entry, src));
            else compile(entry + ".js", script.textContent || "", { exports: {} });
        }
    }
    if (/\.html?$/i.test(entry)) {
        html();
        return;
    }
    if (/\.(svg|md|txt|mmd|mermaid|drawio)$/i.test(entry)) {
        if (/\.svg$/i.test(entry)) {
            const img = new Image();
            img.src = asset(entry);
            document.getElementById("root")!.append(img);
        } else if (/\.(mmd|mermaid)$/i.test(entry)) {
            const mermaid = (
                window as unknown as {
                    mermaid: {
                        initialize: (opts: object) => void;
                        render: (id: string, text: string) => Promise<{ svg: string }>;
                    };
                }
            ).mermaid;
            mermaid.initialize({ startOnLoad: false, securityLevel: "strict" });
            const root = document.getElementById("root")!;
            void mermaid
                .render("diagram", read(entry))
                .then(({ svg }) => {
                    root.innerHTML = svg;
                })
                .catch((e) => {
                    root.textContent = String(e);
                });
        } else document.getElementById("root")!.textContent = read(entry);
        return;
    }
    const exported = load(entry);
    const component = exported.default;
    if (component) {
        if (entry.endsWith(".vue")) Vue.createApp(component).mount("#root");
        else ReactDOM.createRoot(document.getElementById("root")!).render(React.createElement(component));
    }
}
let initialized = false;
window.addEventListener("message", (event) => {
    if (initialized || event.source !== parent || event.data?.type !== "aipp-choice-project") return;
    initialized = true;
    try {
        mountProject(event.data.files, event.data.entry);
    } catch (e) {
        const node = document.createElement("pre");
        node.setAttribute("role", "alert");
        node.textContent = "工程预览失败：" + String(e);
        document.body.append(node);
    }
});
parent.postMessage({ type: "aipp-choice-project-ready" }, "*");
