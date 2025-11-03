import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import React, { useState, useCallback, useEffect, useRef, useMemo } from "react";
import IconButton from "./IconButton";
import Ok from "../assets/ok.svg?react";
import Copy from "../assets/copy.svg?react";
import Run from "../assets/run.svg?react";
import { useCodeTheme } from "../hooks/useCodeTheme";
import { listen } from "@tauri-apps/api/event";
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter";
import { oneDark, oneLight } from "react-syntax-highlighter/dist/esm/styles/prism";
import { useTheme } from "../hooks/useTheme";

// 固定工具栏位置由 IntersectionObserver 控制，无需动态计算偏移

const CodeBlock = React.memo(
    ({
        language,
        children,
        onCodeRun,
    }: {
        language: string;
        children: React.ReactNode;
        onCodeRun: (lang: string, code: string) => void;
    }) => {
        const [copyIconState, setCopyIconState] = useState<"copy" | "ok">("copy");
        const [shouldShowFixed, setShouldShowFixed] = useState(false);
        const codeRef = useRef<HTMLElement>(null);
        const containerRef = useRef<HTMLDivElement>(null);
        const headerSentinelRef = useRef<HTMLDivElement>(null);

        // 获取当前主题信息
        const { currentTheme } = useCodeTheme();
        const { resolvedTheme } = useTheme();
        const [forceUpdate, setForceUpdate] = useState(0);
        const codeString = useMemo(() => (typeof children === 'string' ? children : String(children)), [children]);
        const lineCount = useMemo(() => codeString.split(/\r?\n/).length, [codeString]);
        const isLongCode = codeString.length > 3000 || lineCount > 120;
        const isVeryLongCode = codeString.length > 10000 || lineCount > 300;
        const [collapsed, setCollapsed] = useState<boolean>(isLongCode);

        const getCodeString = useCallback(() => {
            return codeRef.current?.innerText ?? "";
        }, []);

        const handleCopy = useCallback(() => {
            writeText(getCodeString());
            setCopyIconState("ok");
        }, [getCodeString]);

        useEffect(() => {
            if (copyIconState === "ok") {
                const timer = setTimeout(() => {
                    setCopyIconState("copy");
                }, 1500);

                return () => clearTimeout(timer);
            }
        }, [copyIconState]);

        // 使用 IntersectionObserver 判断“原位按钮”是否可见，不再做滚动测量
        useEffect(() => {
            const sentinel = headerSentinelRef.current;
            if (!sentinel) return;
            const observer = new IntersectionObserver(
                (entries) => {
                    const entry = entries[0];
                    // 当顶部 sentinel 不可见时，显示固定按钮
                    setShouldShowFixed(!entry.isIntersecting);
                },
                { root: null, rootMargin: '0px', threshold: [0, 1] }
            );
            observer.observe(sentinel);
            return () => observer.disconnect();
        }, []);

        // 监听主题变化事件
        useEffect(() => {
            const unlistenThemeChange = listen("theme-changed", async (event) => {
                console.log("CodeBlock: Theme change event received:", event.payload);
                // 强制重新渲染以应用新主题
                setForceUpdate((prev) => prev + 1);
            });

            return () => {
                unlistenThemeChange.then((f) => f());
            };
        }, []);

        // 使用 react-syntax-highlighter 进行客户端高亮渲染

        const ButtonGroup = () => (
            <div className="flex items-center gap-1 bg-white/90 opacity-0 group-hover/codeblock:opacity-100 hover:opacity-100 transition-opacity duration-200 rounded-md p-1 backdrop-blur-sm">
                <IconButton
                    icon={copyIconState === "copy" ? <Copy fill="black" /> : <Ok fill="black" />}
                    onClick={handleCopy}
                />
                <IconButton icon={<Run fill="black" />} onClick={() => onCodeRun(language, getCodeString())} />
            </div>
        );

        return (
            <div
                ref={containerRef}
                className="relative rounded-lg group/codeblock border border-border"
                data-theme={currentTheme}
                data-force-update={forceUpdate}
                style={collapsed ? { maxHeight: isVeryLongCode ? 480 : 320, overflow: 'hidden' } : undefined}
            >
                {/* 顶部 sentinel + 普通状态下的按钮 */}
                <div ref={headerSentinelRef} className="absolute left-0 top-0 h-[1px] w-[1px]" />
                <div className="absolute right-2 top-2 z-10">
                    <ButtonGroup />
                </div>

                {/* 滚动时的固定按钮 */}
                {shouldShowFixed && (
                    <div className="fixed z-50 right-2 top-2">
                        <ButtonGroup />
                    </div>
                )}

                <div ref={codeRef as any}>
                    <SyntaxHighlighter
                        language={language}
                        style={resolvedTheme === 'dark' ? (oneDark as any) : (oneLight as any)}
                        PreTag="div"
                    >
                        {codeString}
                    </SyntaxHighlighter>
                </div>

                {isLongCode && (
                    <div className="absolute bottom-0 left-0 right-0 pointer-events-none"
                        style={{
                            background: collapsed ? 'linear-gradient(to top, rgba(0,0,0,0.35), rgba(0,0,0,0) 60%)' : 'none',
                            height: collapsed ? 64 : 0,
                        }}
                    />
                )}

                {isLongCode && (
                    <div className="absolute bottom-2 right-2 z-10">
                        <button
                            className="pointer-events-auto text-xs px-2 py-1 rounded bg-neutral-800/80 text-white hover:bg-neutral-700/90 dark:bg-neutral-200/80 dark:text-black dark:hover:bg-neutral-300/90"
                            onClick={() => setCollapsed((v) => !v)}
                            title={collapsed ? '展开全部' : '收起'}
                        >
                            {collapsed ? '展开全部' : '收起'}
                        </button>
                    </div>
                )}
            </div>
        );
    }
);

export default CodeBlock;
