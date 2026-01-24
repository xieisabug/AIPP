import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { CommandDialog, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from "./ui/command";
import { searchConversations } from "../services/conversationSearchService";
import { ConversationSearchHit } from "../data/Conversation";
import { useAntiLeakage } from "../contexts/AntiLeakageContext";
import { maskContent, maskTitle } from "../utils/antiLeakage";
import { getErrorMessage } from "../utils/error";

interface ConversationSearchDialogProps {
    open: boolean;
    onOpenChange: (open: boolean) => void;
    onSelectResult: (hit: ConversationSearchHit) => void;
}

interface ConversationGroup {
    conversationId: number;
    conversationName: string;
    assistantName: string;
    hits: ConversationSearchHit[];
}

const escapeRegExp = (value: string) => value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

const ConversationSearchDialog = memo(function ConversationSearchDialog({
    open,
    onOpenChange,
    onSelectResult,
}: ConversationSearchDialogProps) {
    const [query, setQuery] = useState("");
    const [isLoading, setIsLoading] = useState(false);
    const [errorMessage, setErrorMessage] = useState<string | null>(null);
    const [hits, setHits] = useState<ConversationSearchHit[]>([]);
    const [hasMore, setHasMore] = useState(false);
    const [emptyMessage, setEmptyMessage] = useState("输入关键字开始搜索");
    const [isLoadingMore, setIsLoadingMore] = useState(false);
    const listRef = useRef<HTMLDivElement | null>(null);
    const debounceRef = useRef<number | null>(null);
    const requestIdRef = useRef(0);
    const { enabled: antiLeakageEnabled, isRevealed } = useAntiLeakage();

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
            const nodes: Array<string | JSX.Element> = [];
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
                        className="rounded bg-primary/20 text-primary px-0.5"
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
        const map = new Map<number, ConversationGroup>();
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
            const results = await searchConversations(trimmed, 50, 0);
            if (requestIdRef.current !== requestId) return;
            setHits(results);
            setHasMore(results.length === 50);
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
    }, []);

    const loadMore = useCallback(async () => {
        if (isLoadingMore || isLoading || !hasMore) {
            return;
        }
        const trimmed = query.trim();
        if (!trimmed) {
            return;
        }
        setIsLoadingMore(true);
        try {
            const results = await searchConversations(trimmed, 50, hits.length);
            if (results.length > 0) {
                setHits((prev) => [...prev, ...results]);
            }
            setHasMore(results.length === 50);
        } catch (error) {
            setErrorMessage(getErrorMessage(error));
            setHasMore(false);
        } finally {
            setIsLoadingMore(false);
        }
    }, [hasMore, hits.length, isLoading, isLoadingMore, query]);

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
        (hit: ConversationSearchHit) => {
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
            description="搜索对话标题、摘要和消息内容"
            className="sm:max-w-3xl"
            showCloseButton={false}
        >
            <CommandInput
                placeholder="搜索对话内容、标题、总结..."
                value={query}
                onValueChange={setQuery}
                autoFocus
            />
            <div
                ref={listRef}
                className="max-h-[420px] overflow-y-auto overflow-x-hidden"
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
                {groupedResults.map((group) => (
                    <div key={group.conversationId} className="px-2">
                        <div className="px-2 py-1.5 text-xs font-medium text-muted-foreground">
                            {renderHighlighted(maskedTitle(group.conversationName))}
                            <span className="mx-1">·</span>
                            {renderHighlighted(maskedTitle(group.assistantName))}
                        </div>
                        {group.hits.map((hit, index) => (
                            <button
                                key={`${hit.conversation_id}-${hit.message_id ?? "summary"}-${index}`}
                                className="w-full text-left px-2 py-2 rounded-md hover:bg-muted/60 transition-colors"
                                onClick={() => handleSelect(hit)}
                                type="button"
                            >
                                <div className="flex items-center gap-2 text-xs text-muted-foreground">
                                    <span className="rounded-full bg-muted px-2 py-0.5 text-[11px]">
                                        {hit.hit_type === "title"
                                            ? "标题"
                                            : hit.hit_type === "summary"
                                                ? "摘要"
                                                : "消息"}
                                    </span>
                                    {hit.message_type && (
                                        <span className="text-[11px]">
                                            {hit.message_type === "user" ? "用户" : hit.message_type}
                                        </span>
                                    )}
                                </div>
                                <div className="text-sm text-foreground line-clamp-2 mt-1">
                                    {renderHighlighted(maskedText(hit.snippet))}
                                </div>
                            </button>
                        ))}
                    </div>
                ))}
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
});

ConversationSearchDialog.displayName = "ConversationSearchDialog";

export default ConversationSearchDialog;
