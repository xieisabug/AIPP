// Bundled by Vite's worker pipeline as a standalone classic script, executed only
// inside an opaque-origin sandboxed iframe. No Tauri imports or host APIs.
import * as React from "react";
import { createRoot } from "react-dom/client";
import * as Vue from "vue/dist/vue.esm-bundler.js";
import { transform } from "@babel/standalone";
import { parse } from "vue/compiler-sfc";

function report(error: unknown) {
    const output = document.getElementById("error")!;
    output.textContent = `组件预览失败：${error instanceof Error ? error.message : String(error)}`;
    output.hidden = false;
}
window.addEventListener("error", event => report(event.error ?? event.message));
window.addEventListener("unhandledrejection", event => report(event.reason));

function compile(source: string, jsx: boolean): unknown {
    const result = transform(source, {
        presets: jsx ? [["react", { runtime: "classic" }]] : [],
        plugins: ["transform-modules-commonjs", () => ({ visitor: {
            Import(path: { buildCodeFrameError: (message: string) => Error }) { throw path.buildCodeFrameError("不支持动态 import"); },
            MetaProperty(path: { buildCodeFrameError: (message: string) => Error }) { throw path.buildCodeFrameError("不支持 import.meta"); },
        } })],
        filename: jsx ? "component.jsx" : "component.js",
    });
    const module = { exports: {} as { default?: unknown } };
    const require = (name: string) => {
        if (name === "react") return React;
        if (name === "vue") return Vue;
        throw new Error(`不支持依赖 ${name}；仅允许 react / vue，不加载网络或 Node 模块`);
    };
    new Function("require", "module", "exports", "React", `${result.code}\n//# sourceURL=mobile-component.js`)(require, module, module.exports, React);
    if (!module.exports.default) throw new Error("组件必须提供 export default");
    return module.exports.default;
}

let initialized = false;
window.addEventListener("message", event => {
    if (event.source !== parent || initialized || event.data?.type !== "aipp-preview-component") return;
    initialized = true;
    parent.postMessage({ type: "aipp-preview-runtime-ready" }, "*");
    try {
        const { language, code } = event.data;
        if (typeof code !== "string" || code.length > 500_000) throw new Error("组件内容无效或超过 500KB 限制");
        const root = document.getElementById("root")!;
        if (language === "react" || language === "jsx") {
            const Component = compile(code, true) as React.ComponentType;
            createRoot(root, { onUncaughtError: report }).render(React.createElement(Component));
        } else if (language === "vue") {
            const { descriptor, errors } = parse(code);
            if (errors.length) throw errors[0];
            if (descriptor.scriptSetup || descriptor.script?.lang || descriptor.template?.lang || descriptor.styles.some(style => style.lang || style.src) || descriptor.script?.src || descriptor.template?.src) {
                throw new Error("Vue 仅支持普通 JavaScript <script>、<template> 和原生 <style>；不支持 script setup、外部文件或预处理器");
            }
            if (!descriptor.template) throw new Error("Vue 组件缺少 <template>");
            const component = descriptor.script ? compile(descriptor.script.content, false) as Record<string, unknown> : {};
            component.template = descriptor.template.content;
            for (const style of descriptor.styles) {
                const element = document.createElement("style"); element.textContent = style.content; document.head.append(element);
            }
            const app = Vue.createApp(component);
            app.config.errorHandler = report;
            app.mount(root);
        } else throw new Error(`不支持组件格式 ${language}`);
    } catch (error) { report(error); }
});
