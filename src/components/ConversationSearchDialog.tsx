import { useDataRefresh } from "@/hooks/useDataRefresh";
import { memo, useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { CommandDialog, CommandInput } from "./ui/command";
import { searchConversations } from "../services/conversationSearchService";
import { ConversationSearchHit } from "../data/Conversation";
import { formatHitTime, HIT_TYPE_LABELS, speakerLabel } from "./conversationSearchFormat";
import { useAntiLeakage } from "../contexts/AntiLeakageContext";
import { maskContent, maskTitle } from "../utils/antiLeakage";
import { getErrorMessage } from "../utils/error";
import { ChevronLeft, CornerDownLeft, MessagesSquare } from "lucide-react";
import { Button } from "./ui/button";
import { useIsMobilePlatform } from "../hooks/use-platform";

interface ConversationSearchDialogProps<T extends ConversationSearchHit = ConversationSearchHit> {
    open: boolean;
    onOpenChange: (open: boolean) => void;
    onSelectResult: (hit: T) => void;
    /** 自定义检索源（如管家消息搜索）；默认搜索普通对话 */
    search?: (query: string, limit: number, offset: number) => Promise<T[]>;
    placeholder?: string;
    description?: string;
}

interface ConversationGroup<T extends ConversationSearchHit> {
    conversationId: number;
    conversationName: string;
    assistantName: string;
    hits: T[];
}

const escapeRegExp = (value: string) => value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

const PAGE_SIZE = 50;

function ConversationSearchDialogImpl<T extends ConversationSearchHit>({
    open,
    onOpenChange,
    onSelectResult,
    search = searchConversations as unknown as NonNullable<ConversationSearchDialogProps<T>["search"]>,
    placeholder = "搜索对话内容、标题、总结...",
    description = "搜索对话标题、摘要和消息内容",
}: ConversationSearchDialogProps<T>) {
    const [query, setQuery] = useState("");
    const [isLoading, setIsLoading] = useState(false);
    const [errorMessage, setErrorMessage] = useState<string | null>(null);
    const [hits, setHits] = useState<T[]>([]);
    const [hasMore, setHasMore] = useState(false);
    const [emptyMessage, setEmptyMessage] = useState("输入关键字开始搜索");
    const [isLoadingMore, setIsLoadingMore] = useState(false);
    const listRef = useRef<HTMLDivElement | null>(null);
    const debounceRef = useRef<number | null>(null);
    const requestIdRef = useRef(0);
    const { enabled: antiLeakageEnabled, isRevealed } = useAntiLeakage();
    // 移动平台对话框被全屏化：需要显式返回入口，结果区占满剩余高度
    const isMobilePlatform = useIsMobilePlatform();

    const shouldMask = antiLeakageEnabled && !isRevealed;
    const highlightTerm = query.trim();

    const maskedText = useCallback(
        (text: string) => (shouldMask ? maskContent(text) : text),
        [shouldMask],
    );

    const maskedTitle = useCallback(
        (text: string) => (shouldMask ? maskTitle(text) : text),
        [shouldMask],
    );

    const renderHighlighted = useCallback(
        (text: string) => {
            if (!highlightTerm) {
                return text;
            }
            const escaped = escapeRegExp(highlightTerm);
            const regex = new RegExp(escaped, "gi");
            const nodes: Array<string | ReactNode> = [];
            let lastIndex = 0;
            let match: RegExpExecArray | null;
            let index = 0;
            let hasMatch = false;
            while ((match = regex.exec(text)) !== null) {
                hasMatch = true;
                const start = match.index;
                const end = start + match[0].length;
                if (start > lastIndex) {
                    nodes.push(text.slice(lastIndex, start));
                }
                nodes.push(
                    <span
                        key={`highlight-${index}`}
                        className="rounded-sm bg-foreground/15 px-0.5 font-semibold text-foreground"
                    >
                        {match[0]}
                    </span>,
                );
                lastIndex = end;
                index += 1;
                if (match.index === regex.lastIndex) {
                    regex.lastIndex += 1;
                }
            }
            if (!hasMatch) {
                return text;
            }
            if (lastIndex < text.length) {
                nodes.push(text.slice(lastIndex));
            }
            return nodes;
        },
        [highlightTerm],
    );

    const groupedResults = useMemo(() => {
        const map = new Map<number, ConversationGroup<T>>();
        hits.forEach((hit) => {
            const existing = map.get(hit.conversation_id);
            if (!existing) {
                map.set(hit.conversation_id, {
                    conversationId: hit.conversation_id,
                    conversationName: hit.conversation_name,
                    assistantName: hit.assistant_name,
                    hits: [hit],
                });
            } else {
                existing.hits.push(hit);
            }
        });
        return Array.from(map.values());
    }, [hits]);

    const runSearch = useCallback(async (searchText: string) => {
        const trimmed = searchText.trim();
        if (!trimmed) {
            setHits([]);
            setErrorMessage(null);
            setIsLoading(false);
            setHasMore(false);
            setEmptyMessage("输入关键字开始搜索");
            return;
        }

        const requestId = requestIdRef.current + 1;
        requestIdRef.current = requestId;
        setIsLoading(true);
        setErrorMessage(null);

        try {
            const results = await search(trimmed, PAGE_SIZE, 0);
            if (requestIdRef.current !== requestId) return;
            setHits(results);
            setHasMore(results.length === PAGE_SIZE);
            setEmptyMessage(results.length === 0 ? "没有找到匹配结果" : "");
        } catch (error) {
            if (requestIdRef.current !== requestId) return;
            setErrorMessage(getErrorMessage(error));
            setHits([]);
            setHasMore(false);
            setEmptyMessage("搜索失败，请重试");
        } finally {
            if (requestIdRef.current === requestId) {
                setIsLoading(false);
            }
        }
    }, [search]);

    useDataRefresh({
        events: ["conversation_data_changed"],
        identity: `${open}:${query}`,
        paused: !open,
        onInvalidate: () => { requestIdRef.current += 1; },
        refresh: async () => { if (open) await runSearch(query); },
    });

    const loadMore = useCallback(async () => {
        if (isLoadingMore || isLoading || !hasMore) {
            return;
        }
        const trimmed = query.trim();
        if (!trimmed) {
            return;
        }
        setIsLoadingMore(true);
        const requestId = requestIdRef.current;
        try {
            const results = await search(trimmed, PAGE_SIZE, hits.length);
            if (requestId !== requestIdRef.current) return;
            if (results.length > 0) {
                setHits((prev) => [...prev, ...results]);
            }
            setHasMore(results.length === PAGE_SIZE);
        } catch (error) {
            setErrorMessage(getErrorMessage(error));
            setHasMore(false);
        } finally {
            setIsLoadingMore(false);
        }
    }, [hasMore, hits.length, isLoading, isLoadingMore, query, search]);

    useEffect(() => {
        if (!open) {
            setQuery("");
            setHits([]);
            setErrorMessage(null);
            setIsLoading(false);
            setHasMore(false);
            setEmptyMessage("输入关键字开始搜索");
            return;
        }
    }, [open]);

    useEffect(() => {
        if (!open) {
            return;
        }
        if (debounceRef.current) {
            window.clearTimeout(debounceRef.current);
        }
        debounceRef.current = window.setTimeout(() => {
            runSearch(query);
        }, 250);
        return () => {
            if (debounceRef.current) {
                window.clearTimeout(debounceRef.current);
            }
        };
    }, [open, query, runSearch]);

    const handleSelect = useCallback(
        (hit: T) => {
            onSelectResult(hit);
            onOpenChange(false);
        },
        [onSelectResult, onOpenChange],
    );

    return (
        <CommandDialog
            open={open}
            onOpenChange={onOpenChange}
            title="搜索"
            description={description}
            className={isMobilePlatform ? "flex flex-col gap-0" : "sm:max-w-3xl"}
            showCloseButton={false}
        >
            {isMobilePlatform ? (
                <div
                    className="flex shrink-0 items-center gap-1 border-b pr-1 [&_[data-slot=command-input-wrapper]]:flex-1 [&_[data-slot=command-input-wrapper]]:border-b-0 [&_[data-slot=command-input-wrapper]]:px-1"
                    data-aipp-slot="search-mobile-header"
                >
                    <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        aria-label="返回"
                        onClick={() => onOpenChange(false)}
                    >
                        <ChevronLeft className="h-5 w-5" />
                    </Button>
                    <CommandInput
                        placeholder={placeholder}
                        value={query}
                        onValueChange={setQuery}
                        enterKeyHint="search"
                        autoFocus
                    />
                </div>
            ) : (
                <CommandInput
                    placeholder={placeholder}
                    value={query}
                    onValueChange={setQuery}
                    autoFocus
                />
            )}
            <div
                ref={listRef}
                className={isMobilePlatform
                    ? "min-h-0 flex-1 overflow-y-auto overflow-x-hidden"
                    : "max-h-[420px] overflow-y-auto overflow-x-hidden"}
                onScroll={(event) => {
                    const target = event.currentTarget;
                    if (target.scrollTop + target.clientHeight >= target.scrollHeight - 24) {
                        loadMore();
                    }
                }}
            >
                {isLoading && (
                    <div className="py-6 text-center text-sm text-muted-foreground">
                        正在搜索...
                    </div>
                )}
                {errorMessage && (
                    <div className="py-6 text-center text-sm text-destructive">
                        {errorMessage}
                    </div>
                )}
                {!isLoading && !errorMessage && groupedResults.length === 0 && (
                    <div className="py-6 text-center text-sm text-muted-foreground">
                        {emptyMessage}
                    </div>
                )}
                {groupedResults.length > 0 && (
                    <div className="space-y-3 p-3" data-aipp-slot="search-results">
                        {groupedResults.map((group) => (
                            <section
                                key={group.conversationId}
                                className="overflow-hidden rounded-lg border border-border"
                                data-aipp-slot="search-result-group"
                            >
                                <header className="flex items-center gap-2 border-b border-border bg-muted/50 px-3 py-2">
                                    <MessagesSquare className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
                                    <span className="min-w-0 truncate text-xs font-semibold text-foreground">
                                        {renderHighlighted(maskedTitle(group.conversationName))}
                                    </span>
                                    <span className="min-w-0 truncate text-xs text-muted-foreground">
                                        {renderHighlighted(maskedTitle(group.assistantName))}
                                    </span>
                                    <span className="ml-auto shrink-0 text-[11px] tabular-nums text-muted-foreground">
                                        {group.hits.length} 条
                                    </span>
                                </header>
                                <ul className="divide-y divide-border">
                                    {group.hits.map((hit, index) => (
                                        <li key={`${hit.conversation_id}-${hit.message_id ?? "summary"}-${index}`}>
                                            <button
                                                className={`group block w-full px-3 py-2.5 text-left transition-colors hover:bg-muted/60 focus-visible:bg-muted/60 focus-visible:outline-none${isMobilePlatform ? " min-h-11 active:bg-muted/60" : ""}`}
                                                onClick={() => handleSelect(hit)}
                                                type="button"
                                            >
                                                <div className="flex items-center gap-2 text-[11px] text-muted-foreground">
                                                    <span className="shrink-0 rounded border border-border px-1.5 leading-4">
                                                        {HIT_TYPE_LABELS[hit.hit_type]}
                                                    </span>
                                                    {speakerLabel(hit) && (
                                                        <span className="min-w-0 truncate font-medium text-foreground/80">
                                                            {speakerLabel(hit)}
                                                        </span>
                                                    )}
                                                    <span className="ml-auto shrink-0 tabular-nums">
                                                        {formatHitTime(hit.created_time)}
                                                    </span>
                                                    <CornerDownLeft className="h-3 w-3 shrink-0 opacity-0 transition-opacity group-hover:opacity-100 group-focus-visible:opacity-100" />
                                                </div>
                                                <div className="mt-1 line-clamp-2 break-words text-sm leading-relaxed text-foreground">
                                                    {renderHighlighted(maskedText(hit.snippet))}
                                                </div>
                                            </button>
                                        </li>
                                    ))}
                                </ul>
                            </section>
                        ))}
                    </div>
                )}
                {isLoadingMore && (
                    <div className="py-4 text-center text-xs text-muted-foreground">
                        加载更多...
                    </div>
                )}
                {!isLoadingMore && hasMore && groupedResults.length > 0 && (
                    <div className="py-2 text-center text-xs text-muted-foreground">
                        滚动加载更多
                    </div>
                )}
            </div>
        </CommandDialog>
    );
}

const ConversationSearchDialog = memo(ConversationSearchDialogImpl) as typeof ConversationSearchDialogImpl;

export default ConversationSearchDialog;
