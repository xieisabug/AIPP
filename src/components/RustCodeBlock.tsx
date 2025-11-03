import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { useTheme } from "@/hooks/useTheme";
import IconButton from "./IconButton";
import Ok from "../assets/ok.svg?react";
import Copy from "../assets/copy.svg?react";
import Run from "../assets/run.svg?react";
import { useRustHighlight } from "@/hooks/highlight/useRustHighlight";
import { useCodeTheme } from "@/hooks/useCodeTheme";

interface RustCodeBlockProps {
    language: string;
    children: React.ReactNode; // code string
    onCodeRun?: (lang: string, code: string) => void;
    className?: string;
}

const RustCodeBlock: React.FC<RustCodeBlockProps> = ({ language, children, onCodeRun, className = "" }) => {
    const code = useMemo(() => (typeof children === "string" ? children : String(children)), [children]);
    const { resolvedTheme } = useTheme();
    const [html, setHtml] = useState<string>("");
    const [copyState, setCopyState] = useState<"copy" | "ok">("copy");
    const codeRef = useRef<HTMLDivElement | null>(null);
    const { currentTheme } = useCodeTheme();
    const rustHighlight = useRustHighlight();

    useEffect(() => {
        let cancelled = false;
        (async () => {
            try {
                const result = await rustHighlight(language, code, resolvedTheme === "dark", currentTheme);
                if (!cancelled) setHtml(result);
            } catch (e) {
                console.warn("[RustCodeBlock] highlight failed, fallback to plain text", e);
                if (!cancelled) setHtml("");
            }
        })();
        return () => {
            cancelled = true;
        };
    }, [language, code, resolvedTheme, currentTheme]);

    const handleCopy = useCallback(() => {
        writeText(code);
        setCopyState("ok");
        setTimeout(() => setCopyState("copy"), 1500);
    }, [code]);

    return (
        <div className={`relative rounded-lg border border-border bg-background ${className}`}>
            {/* Toolbar */}
            <div className="absolute right-2 top-2 z-10 flex items-center gap-1 bg-white/90 dark:bg-neutral-800/80 rounded p-1 backdrop-blur-sm">
                <IconButton
                    icon={copyState === "copy" ? <Copy fill="black" /> : <Ok fill="black" />}
                    onClick={handleCopy}
                />
                <IconButton icon={<Run fill="black" />} onClick={() => onCodeRun?.(language, code)} />
            </div>

            {/* Highlighted HTML from Rust */}
            {html ? (
                <div
                    ref={codeRef}
                    className="overflow-auto text-sm leading-6 font-mono"
                    dangerouslySetInnerHTML={{ __html: html }}
                />
            ) : (
                <pre className="overflow-auto text-sm leading-6 font-mono p-3">
                    <code>{code}</code>
                </pre>
            )}
        </div>
    );
};

export default React.memo(RustCodeBlock);
