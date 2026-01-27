import { visit } from "unist-util-visit";
import { Node } from "unist";

interface CodeNode extends Node {
    type: "code";
    lang?: string | null;
    meta?: string | null;
    data?: {
        hProperties?: Record<string, unknown>;
    };
}

const META_ATTR_REGEX = /([A-Za-z0-9_-]+)(?:\s*=\s*("[^"]*"|'[^']*'|[^\s"']+))?/g;

const parseMetaAttributes = (meta: string): Record<string, string | boolean> => {
    const attributes: Record<string, string | boolean> = {};
    let match: RegExpExecArray | null;

    META_ATTR_REGEX.lastIndex = 0;
    while ((match = META_ATTR_REGEX.exec(meta)) !== null) {
        const rawKey = match[1];
        const rawValue = match[2];
        const key = rawKey.toLowerCase();

        if (!rawValue) {
            attributes[key] = true;
            continue;
        }

        const trimmed = rawValue.trim();
        const unquoted =
            (trimmed.startsWith('"') && trimmed.endsWith('"')) ||
            (trimmed.startsWith("'") && trimmed.endsWith("'"))
                ? trimmed.slice(1, -1)
                : trimmed;
        attributes[key] = unquoted;
    }

    return attributes;
};

export interface CodeBlockMetaInfo {
    meta?: string;
    title?: string;
    filename?: string;
    line?: string;
    highlight?: string;
}

export const parseCodeBlockMeta = (meta: string): CodeBlockMetaInfo => {
    const trimmed = meta.trim();
    if (!trimmed) return {};
    const parsed = parseMetaAttributes(trimmed);
    const title = (parsed.title as string | undefined) ??
        (parsed.filename as string | undefined) ??
        (parsed.file as string | undefined);
    const filename = (parsed.filename as string | undefined) ??
        (parsed.file as string | undefined);
    const line = typeof parsed.line === "string" ? parsed.line : undefined;
    const highlight = typeof parsed.highlight === "string" ? parsed.highlight : undefined;

    return {
        meta: trimmed,
        title,
        filename,
        line,
        highlight,
    };
};

const getStringProp = (value: unknown): string | undefined =>
    typeof value === "string" ? value : undefined;

export const resolveCodeBlockMeta = (
    props: Record<string, unknown>,
    node?: Node | null,
): CodeBlockMetaInfo | null => {
    const title = getStringProp(props["data-title"]);
    const filename = getStringProp(props["data-filename"]);
    const line = getStringProp(props["data-line"]);
    const highlight = getStringProp(props["data-highlight"]);
    const metaFromProps = getStringProp(props["data-meta"]);

    const nodeMeta = getStringProp((node as { data?: { meta?: unknown } } | undefined)?.data?.meta);
    const meta = metaFromProps ?? nodeMeta;

    if (title || filename || line || highlight || meta) {
        if (title || filename || line || highlight) {
            return {
                meta,
                title,
                filename,
                line,
                highlight,
            };
        }
        if (meta) {
            return parseCodeBlockMeta(meta);
        }
    }

    return null;
};

export const buildCodeBlockMetaAttributes = (meta?: CodeBlockMetaInfo | null) => {
    if (!meta) return {};
    return {
        ...(meta.meta ? { "data-meta": meta.meta } : {}),
        ...(meta.title ? { "data-title": meta.title } : {}),
        ...(meta.filename ? { "data-filename": meta.filename } : {}),
        ...(meta.line ? { "data-line": meta.line } : {}),
        ...(meta.highlight ? { "data-highlight": meta.highlight } : {}),
    };
};

export default function remarkCodeBlockMeta() {
    return (tree: Node) => {
        visit(tree, "code", (node: CodeNode) => {
            const meta = node.meta?.trim();
            const parsed = meta ? parseCodeBlockMeta(meta) : null;

            if (!meta && !node.lang) return;

            const data = node.data || {};
            data.hProperties = {
                ...(data.hProperties || {}),
                ...(node.lang ? { "data-language": node.lang } : {}),
                ...(parsed ? buildCodeBlockMetaAttributes(parsed) : {}),
            };
            node.data = data;
        });
    };
}
