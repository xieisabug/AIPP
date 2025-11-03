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
    const [isHovered, setIsHovered] = useState(false);
    const [isSticky, setIsSticky] = useState(false);
    const [toolbarRight, setToolbarRight] = useState<number>(8);
    const containerRef = useRef<HTMLDivElement | null>(null);
    const codeRef = useRef<HTMLDivElement | null>(null);
    const rafRef = useRef<number | null>(null);
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

    // 监听滚动判断是否需要 sticky - 使用 RAF 节流
    useEffect(() => {
        if (!isHovered) {
            setIsSticky(false);
            return;
        }

        const handleScroll = () => {
            // 取消之前的 RAF
            if (rafRef.current) {
                cancelAnimationFrame(rafRef.current);
            }

            // 使用 RAF 节流，确保在下一帧才更新
            rafRef.current = requestAnimationFrame(() => {
                if (!containerRef.current) return;

                const rect = containerRef.current.getBoundingClientRect();
                const shouldStick = rect.top < 8 && rect.bottom > 60;
                
                // 只在状态真正变化时才更新
                setIsSticky(prev => {
                    if (prev !== shouldStick) {
                        // 同时更新 right 位置，避免额外渲染
                        if (shouldStick) {
                            setToolbarRight(window.innerWidth - rect.right + 8);
                        }
                        return shouldStick;
                    }
                    return prev;
                });
            });
        };

        window.addEventListener("scroll", handleScroll, { passive: true });
        const scrollParent = containerRef.current?.closest('.overflow-auto, .overflow-y-auto');
        scrollParent?.addEventListener("scroll", handleScroll, { passive: true } as any);
        
        handleScroll(); // 初始检查

        return () => {
            if (rafRef.current) {
                cancelAnimationFrame(rafRef.current);
            }
            window.removeEventListener("scroll", handleScroll);
            scrollParent?.removeEventListener("scroll", handleScroll as any);
        };
    }, [isHovered]);

    const handleCopy = useCallback(() => {
        writeText(code);
        setCopyState("ok");
        setTimeout(() => setCopyState("copy"), 1500);
    }, [code]);

    return (
        <div 
            ref={containerRef}
            className={`relative rounded-lg border border-border bg-background ${className}`}
            onMouseEnter={() => setIsHovered(true)}
            onMouseLeave={() => setIsHovered(false)}
        >
            {/* Toolbar - 根据 isSticky 切换定位方式 */}
            <div 
                className={`
                    z-50
                    flex items-center gap-1 
                    bg-white/90 dark:bg-neutral-800/80 rounded p-1 backdrop-blur-sm
                    transition-opacity duration-150 ease-out
                    will-change-opacity
                    ${isSticky ? 'fixed top-2 shadow-lg' : 'absolute right-2 top-2'}
                    ${isHovered ? 'opacity-100' : 'opacity-0 pointer-events-none'}
                `}
                style={isSticky ? { right: `${toolbarRight}px` } : undefined}
            >
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
