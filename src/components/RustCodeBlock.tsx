import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { useTheme } from "@/hooks/useTheme";
import IconButton from "./IconButton";
import { Copy, Check, SquareTerminal } from "lucide-react";
import { getCachedRustHighlight, useRustHighlight } from "@/hooks/highlight/useRustHighlight";
import { useCodeTheme } from "@/hooks/useCodeTheme";
import { useFeatureAvailableOnPlatform } from "@/lib/mobileUnsupported";
import type { CodeBlockMetaInfo } from "@/react-markdown/remarkCodeBlockMeta";

interface RustCodeBlockProps {
    language: string;
    children: React.ReactNode; // code string
    onCodeRun?: (lang: string, code: string) => void;
    className?: string;
    // 是否处于大模型流式输出中（用于首次阈值超限时自动折叠）
    isStreaming?: boolean;
    meta?: CodeBlockMetaInfo | null;
    disableCollapse?: boolean;
}

const COLLAPSED_MAX_HEIGHT = 320;
const INITIAL_COLLAPSE_WRAP_CHARS = 96;
const INITIAL_COLLAPSE_LINE_THRESHOLD = 18;
const COLLAPSED_PREVIEW_MAX_LINES = 120;
const COLLAPSED_PREVIEW_MAX_CHARS = 8000;
const PLAIN_TEXT_LANGUAGES = new Set(["", "text", "txt", "plain", "plaintext"]);

function shouldStartCollapsed(code: string): boolean {
    const visualLineCount = code.split(/\r?\n/).reduce((total, line) => {
        return total + Math.max(1, Math.ceil(line.length / INITIAL_COLLAPSE_WRAP_CHARS));
    }, 0);

    return visualLineCount > INITIAL_COLLAPSE_LINE_THRESHOLD;
}

function getCollapsedPreviewCode(code: string): { code: string; truncated: boolean } {
    const lines = code.split(/\r?\n/);
    const lineLimited = lines.length > COLLAPSED_PREVIEW_MAX_LINES;
    let preview = lineLimited
        ? lines.slice(0, COLLAPSED_PREVIEW_MAX_LINES).join("\n")
        : code;

    const charLimited = preview.length > COLLAPSED_PREVIEW_MAX_CHARS;
    if (charLimited) {
        preview = preview.slice(0, COLLAPSED_PREVIEW_MAX_CHARS);
    }

    return {
        code: preview,
        truncated: lineLimited || charLimited || preview.length < code.length,
    };
}

function isPlainTextLanguage(language: string): boolean {
    return PLAIN_TEXT_LANGUAGES.has(language.trim().toLowerCase());
}

const RustCodeBlock: React.FC<RustCodeBlockProps> = ({
    language,
    children,
    onCodeRun,
    className = "",
    isStreaming = false,
    meta = null,
    disableCollapse = false,
}) => {
    const code = useMemo(() => (typeof children === "string" ? children : String(children)), [children]);
    const { resolvedTheme } = useTheme();
    // 脚本执行依赖 shell 环境，移动端不支持，隐藏"运行"按钮
    const codeRunAvailable = useFeatureAvailableOnPlatform("script_execution");
    const [highlighted, setHighlighted] = useState<{ key: string; html: string } | null>(null);
    const [copyState, setCopyState] = useState<"copy" | "ok">("copy");
    const [isHovered, setIsHovered] = useState(false);
    const [isSticky, setIsSticky] = useState(false);
    const [toolbarRight, setToolbarRight] = useState<number>(8);
    const containerRef = useRef<HTMLDivElement | null>(null);
    const codeRef = useRef<HTMLDivElement | null>(null);
    const rafRef = useRef<number | null>(null);
    const { currentTheme } = useCodeTheme();
    const rustHighlight = useRustHighlight();
    const shouldUsePlainText = isPlainTextLanguage(language);

    // 折叠逻辑相关
    const shouldCollapseInitially = useMemo(() => shouldStartCollapsed(code), [code]);
    const [isCollapsed, setIsCollapsed] = useState(
        () => !disableCollapse && shouldCollapseInitially,
    );
    const [isOverflow, setIsOverflow] = useState(false);
    const userToggledRef = useRef(false);
    const streamingAutoCollapsedOnceRef = useRef(false);
    const hasInitialDecisionRef = useRef(false); // 非流式时仅在首次渲染做一次自动判断
    const collapsedPreview = useMemo(() => getCollapsedPreviewCode(code), [code]);
    const renderCode = !disableCollapse && isCollapsed ? collapsedPreview.code : code;
    const highlightKey = useMemo(
        () => JSON.stringify([language, renderCode, resolvedTheme, currentTheme]),
        [language, renderCode, resolvedTheme, currentTheme],
    );
    const cachedHtml = useMemo(
        () => shouldUsePlainText ? undefined : getCachedRustHighlight(language, renderCode, resolvedTheme === "dark", currentTheme),
        [language, renderCode, resolvedTheme, currentTheme, shouldUsePlainText],
    );
    const html = shouldUsePlainText ? "" : cachedHtml
        ?? (highlighted?.key === highlightKey ? highlighted.html : "");
    const isPreviewTruncated = !disableCollapse && isCollapsed && collapsedPreview.truncated;
    const canCollapse = !disableCollapse && (isOverflow || isPreviewTruncated || shouldCollapseInitially);
    const metaLabel = useMemo(() => {
        if (!meta) return null;
        const title = meta.title || meta.filename;
        const parts = [];
        if (title) parts.push(title);
        if (meta.line) parts.push(`line ${meta.line}`);
        if (meta.highlight) parts.push(`highlight ${meta.highlight}`);
        return parts.length ? parts.join(" · ") : null;
    }, [meta]);

    useEffect(() => {
        if (shouldUsePlainText) {
            return;
        }

        if (cachedHtml !== undefined) return;

        let cancelled = false;
        (async () => {
            try {
                const result = await rustHighlight(language, renderCode, resolvedTheme === "dark", currentTheme);
                if (!cancelled) setHighlighted({ key: highlightKey, html: result });
            } catch (e) {
                console.warn("[RustCodeBlock] highlight failed, fallback to plain text", e);
                if (!cancelled) setHighlighted(null);
            }
        })();
        return () => {
            cancelled = true;
        };
    }, [language, renderCode, resolvedTheme, currentTheme, shouldUsePlainText, rustHighlight, cachedHtml, highlightKey]);

    // 计算是否超出折叠阈值，并在需要时进行自动折叠
    useEffect(() => {
        if (disableCollapse) {
            setIsCollapsed(false);
            setIsOverflow(false);
            return;
        }

        const el = codeRef.current;
        if (!el) return;

        const measure = (contentHeight: number) => {
            const overflow = contentHeight > COLLAPSED_MAX_HEIGHT + 4 || shouldCollapseInitially; // 允许少量误差
            setIsOverflow(overflow);

            // 用户手动切换后，不再自动改变折叠状态
            if (userToggledRef.current) return;

            if (isStreaming) {
                // 流式场景：仅在第一次超过阈值时自动收起
                if (overflow && !streamingAutoCollapsedOnceRef.current) {
                    setIsCollapsed(true);
                    streamingAutoCollapsedOnceRef.current = true;
                    // 一旦在流式阶段自动折叠，视为已完成初始决策，避免流结束时状态闪烁
                    hasInitialDecisionRef.current = true;
                }
            } else {
                // 如果在流式阶段已经自动折叠过，则保持当前状态，不再自动调整
                if (streamingAutoCollapsedOnceRef.current) {
                    return;
                }
                // 非流式场景：首次渲染时根据是否溢出设定初始折叠状态
                if (!hasInitialDecisionRef.current) {
                    setIsCollapsed(overflow);
                    hasInitialDecisionRef.current = true;
                }
            }
        };

        // The inner code element is not height-clamped (its parent is). Observe
        // its laid-out border box instead of forcing layout with scrollHeight in
        // every code block's passive effect after each highlight result.
        const observer = new ResizeObserver((entries) => {
            const entry = entries[0];
            if (entry) {
                measure(entry.borderBoxSize[0]?.blockSize ?? entry.contentRect.height);
            }
        });
        observer.observe(el, { box: "border-box" });
        return () => {
            observer.disconnect();
        };
        // 依赖 html 与 code，在代码或高亮结果变化时重新测量
    }, [html, code, renderCode, isStreaming, disableCollapse, shouldCollapseInitially]);

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

    const toggleCollapse = useCallback(() => {
        // 标记用户手动切换
        userToggledRef.current = true;
        setIsCollapsed((prev) => !prev);
    }, []);

    return (
        <div 
            ref={containerRef}
            className={`relative overflow-hidden bg-transparent ${className}`}
            data-meta={meta?.meta}
            data-title={meta?.title}
            data-filename={meta?.filename}
            data-line={meta?.line}
            data-highlight={meta?.highlight}
            onMouseEnter={() => setIsHovered(true)}
            onMouseLeave={() => setIsHovered(false)}
        >
            {/* Toolbar - 根据 isSticky 切换定位方式 */}
            <div 
                className={`
                    z-10
                    flex items-center gap-1 
                    bg-white/85 dark:bg-neutral-800/75 rounded-md p-1 backdrop-blur-sm shadow-sm
                    transition-opacity duration-150 ease-out
                    will-change-opacity
                    ${isSticky ? 'fixed top-20 shadow-lg' : 'absolute right-2 top-2'}
                    ${isHovered ? 'opacity-100' : 'opacity-0 pointer-events-none'}
                `}
                style={isSticky ? { right: `${toolbarRight}px` } : undefined}
            >
                <IconButton
                    icon={copyState === "copy" ? <Copy size={16} className="text-icon" /> : <Check size={16} className="text-icon" />}
                    onClick={handleCopy}
                />
                {codeRunAvailable && (
                    <IconButton icon={<SquareTerminal size={16} className="text-icon" />} onClick={() => onCodeRun?.(language, code)} />
                )}
            </div>

            {metaLabel && (
                <div
                    className="px-3 pt-2 text-xs text-muted-foreground font-mono truncate pr-12"
                    title={metaLabel}
                >
                    {metaLabel}
                </div>
            )}

            {/* Code content with collapsible container */}
            <div
                className="relative"
                style={{
                    maxHeight: !disableCollapse && isCollapsed ? COLLAPSED_MAX_HEIGHT : undefined,
                    overflow: !disableCollapse && isCollapsed ? 'hidden' : 'auto',
                }}
            >
                {html ? (
                    <div
                        ref={codeRef}
                        className="aipp-code-content text-sm font-mono leading-5"
                        dangerouslySetInnerHTML={{ __html: html }}
                    />
                ) : (
                    <div ref={codeRef} className="aipp-code-content text-sm font-mono leading-5">
                        <pre><code>{renderCode}</code></pre>
                    </div>
                )}
                {/* Gradient overlay when collapsed */}
                {!disableCollapse && isCollapsed && canCollapse && (
                    <div className="absolute inset-x-0 bottom-0 h-16 bg-gradient-to-t from-muted to-transparent pointer-events-none" />
                )}
            </div>

            {/* Expand/Collapse control */}
            {canCollapse && (
                <div className="flex justify-center pt-2 pb-1 bg-muted">
                    <button
                        type="button"
                        className="px-3 py-1 text-xs text-foreground/70 hover:text-foreground transition-colors cursor-pointer"
                        onClick={toggleCollapse}
                        aria-label={isCollapsed ? '展开代码' : '收起代码'}
                    >
                        {isCollapsed ? '展开' : '收起'}
                    </button>
                </div>
            )}
        </div>
    );
};

export default React.memo(RustCodeBlock);
