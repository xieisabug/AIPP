import { useCallback, useEffect, useRef, useState, memo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { useDataRefresh } from "@/hooks/useDataRefresh";
import { listen } from "@tauri-apps/api/event";
import { CircleCheckBig, EllipsisVertical, Loader2 } from "lucide-react";
import ConversationTitleEditDialog from "./ConversationTitleEditDialog";
import useConversationManager from "../hooks/useConversationManager";
import { useConversationListStatus } from "../hooks/useConversationListStatus";
import { Conversation, ConversationListItemStatus } from "../data/Conversation";
import { useAntiLeakage } from "../contexts/AntiLeakageContext";
import { maskTitle } from "../utils/antiLeakage";
import {
    DropdownMenu,
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuTrigger,
} from "./ui/dropdown-menu";
import { Button } from "./ui/button";
import ConfirmDialog from "./ConfirmDialog";

interface ConversationItemProps {
    conversation: Conversation;
    isSelected: boolean;
    status: ConversationListItemStatus;
    onSelect: (id: string) => void;
    onOpenTitleEdit: (id: number, title: string) => void;
    onOpenDelete: (id: string, name: string) => void;
}

const ConversationItem = memo(function ConversationItem({
    conversation,
    isSelected,
    status,
    onSelect,
    onOpenTitleEdit,
    onOpenDelete,
}: ConversationItemProps) {
    const { enabled: antiLeakageEnabled, isRevealed } = useAntiLeakage();

    // 判断是否需要脱敏
    const shouldMask = antiLeakageEnabled && !isRevealed;
    const displayName = shouldMask ? maskTitle(conversation.name) : conversation.name;
    const displayAssistantName = shouldMask ? maskTitle(conversation.assistant_name) : conversation.assistant_name;

    return (
        <li
            className={`group h-16 w-full mx-0 mb-2 text-sm border-0 rounded-xl cursor-pointer select-none flex flex-col justify-center p-3 box-border relative transition-all duration-200 ${isSelected ? "font-bold text-primary bg-primary-foreground" : "bg-transparent hover:bg-muted hover:translate-x-0.5"}`}
            data-aipp-slot="chat-conversation-list-item"
            onClick={() => {
                onSelect(conversation.id.toString());
            }}
        >
            <div className="overflow-hidden text-ellipsis whitespace-nowrap font-medium" data-aipp-slot="chat-conversation-item-title">
                {displayName}
            </div>
            <div className="text-xs overflow-hidden text-ellipsis whitespace-nowrap text-muted-foreground" data-aipp-slot="chat-conversation-item-subtitle">
                {displayAssistantName}
            </div>

            <div className="absolute right-2 top-1/2 size-9 -translate-y-1/2">
                {status === "responding" && (
                    <div
                        className="pointer-events-none absolute inset-0 flex items-center justify-center group-hover:hidden"
                        data-aipp-slot="chat-conversation-item-status-responding"
                    >
                        <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
                    </div>
                )}
                {status === "completed_unread" && (
                    <div
                        className="pointer-events-none absolute inset-0 flex items-center justify-center group-hover:hidden"
                        data-aipp-slot="chat-conversation-item-status-completed-unread"
                    >
                        <CircleCheckBig className="h-4 w-4 text-muted-foreground" />
                    </div>
                )}

                <DropdownMenu>
                    <DropdownMenuTrigger asChild>
                        <Button
                            variant="link"
                            size="icon"
                            className="invisible absolute inset-0 h-9 w-9 p-0 group-hover:visible transition-opacity duration-200"
                            data-aipp-slot="chat-conversation-item-menu-trigger"
                            onClick={(e) => {
                                // 避免点击菜单按钮时触发列表项选择
                                e.stopPropagation();
                            }}
                        >
                            <EllipsisVertical className="h-4 w-4 text-icon" />
                        </Button>
                    </DropdownMenuTrigger>
                    <DropdownMenuContent
                        data-aipp-slot="chat-conversation-item-menu"
                        onClick={(e) => {
                            // 阻止菜单内部点击冒泡到列表项
                            e.stopPropagation();
                        }}
                    >
                        <DropdownMenuItem
                            data-aipp-slot="chat-conversation-item-menu-edit"
                            onClick={(e) => {
                                e.stopPropagation();
                                onOpenTitleEdit(conversation.id, conversation.name);
                            }}
                        >
                            修改标题
                        </DropdownMenuItem>
                        <DropdownMenuItem
                            data-aipp-slot="chat-conversation-item-menu-delete"
                            onClick={(e) => {
                                e.stopPropagation();
                                onOpenDelete(
                                    conversation.id.toString(),
                                    conversation.name,
                                );
                            }}
                        >
                            删除
                        </DropdownMenuItem>
                    </DropdownMenuContent>
                </DropdownMenu>
            </div>
        </li>
    );
});

interface ConversationListProps {
    onSelectConversation: (conversation: string) => void;
    conversationId: string;
}

const ConversationList = memo(function ConversationList({
    onSelectConversation,
    conversationId,
}: ConversationListProps) {
    const { getItemStatus } = useConversationListStatus(conversationId);
    const [conversations, setConversations] = useState<Array<Conversation>>([]);

    const [isLoading, setIsLoading] = useState(false);
    const [hasMoreData, setHasMoreData] = useState(true);
    const [isLoadingMore, setIsLoadingMore] = useState(false);
    const scrollContainerRef = useRef<HTMLDivElement>(null);
    const { deleteConversation, listConversations } = useConversationManager();

    const pageCountRef = useRef(1);
    const refreshList = useDataRefresh({
        events: ["conversation_data_changed", "conversation_created", "conversation_deleted"],
        identity: conversationId,
        initial: true,
        refresh: async (isCurrent) => {
            setIsLoading((value) => conversations.length === 0 ? true : value);
            try {
                const pageCount = pageCountRef.current;
                const rows = await listConversations(1, pageCount * 100);
                if (!isCurrent()) return;
                // A selected conversation can lie outside the loaded range.
                if (conversationId && !rows.some((row) => String(row.id) === conversationId)) {
                    const exists = await invoke<boolean>("conversation_exists", { conversationId: +conversationId });
                    if (!isCurrent()) return;
                    if (!exists) onSelectConversation(rows[0]?.id.toString() ?? "");
                }
                const editing = editingDialogStateRef.current;
                if (editing.isOpen) {
                    const updated = rows.find((row) => row.id === editing.id);
                    if (updated) setEditingConversationTitle(updated.name);
                }
                setConversations(rows);
                setHasMoreData(rows.length === pageCount * 100);
            } finally {
                if (isCurrent()) { setIsLoading(false); setIsLoadingMore(false); }
            }
        },
        onError: (error) => {
            setIsLoading(false);
            setIsLoadingMore(false);
            toast.error(`刷新聊天列表失败: ${String(error)}`);
        },
    });

    // Reload the loaded prefix instead of appending pages fetched against an old ordering.
    const loadNextPage = useCallback(() => {
        if (isLoadingMore || isLoading || !hasMoreData) return;
        setIsLoadingMore(true);
        pageCountRef.current += 1;
        refreshList();
    }, [hasMoreData, isLoading, isLoadingMore, refreshList]);

    // 滚动监听，检测是否滚动到最后10%
    useEffect(() => {
        const container = scrollContainerRef.current;
        if (!container) return;

        const handleScroll = () => {
            const { scrollTop, scrollHeight, clientHeight } = container;
            const scrollPercentage = (scrollTop + clientHeight) / scrollHeight;

            // 当滚动到最后10%时加载下一页
            if (scrollPercentage >= 0.9 && hasMoreData && !isLoadingMore) {
                loadNextPage();
            }
        };

        container.addEventListener("scroll", handleScroll);
        return () => container.removeEventListener("scroll", handleScroll);
    }, [hasMoreData, isLoadingMore, loadNextPage]);

    const [titleEditDialogIsOpen, setTitleEditDialogIsOpen] =
        useState<boolean>(false);
    const [editingConversationId, setEditingConversationId] =
        useState<number>(0);
    const [editingConversationTitle, setEditingConversationTitle] =
        useState<string>("");
    const editingDialogStateRef = useRef({ id: 0, isOpen: false });

    useEffect(() => {
        editingDialogStateRef.current = {
            id: editingConversationId,
            isOpen: titleEditDialogIsOpen,
        };
    }, [editingConversationId, titleEditDialogIsOpen]);

    const openTitleEditDialog = useCallback((id: number, title: string) => {
        setEditingConversationId(id);
        setEditingConversationTitle(title || "");
        setTitleEditDialogIsOpen(true);
    }, []);

    const closeTitleEditDialog = useCallback(() => {
        setTitleEditDialogIsOpen(false);
        setEditingConversationId(0);
        setEditingConversationTitle("");
    }, []);

    // 监听标题变化事件
    useEffect(() => {
        let unlisten: (() => void) | undefined;

        const setupListener = async () => {
            unlisten = await listen("title_change", (event) => {
                const [conversationIdFromEvent, title] = event.payload as [
                    number,
                    string,
                ];

                setConversations((prev) => {
                    const index = prev.findIndex(
                        (conversation) =>
                            conversation.id === conversationIdFromEvent,
                    );
                    if (index === -1) return prev;

                    const updated = [...prev];
                    updated[index] = {
                        ...updated[index],
                        name: title,
                    };
                    return updated;
                });

                const { id, isOpen } = editingDialogStateRef.current;
                if (isOpen && id === conversationIdFromEvent) {
                    setEditingConversationTitle(title);
                }
            });
        };

        setupListener();

        return () => {
            if (unlisten) {
                unlisten();
            }
        };
    }, []);

    const [deleteDialogIsOpen, setDeleteDialogIsOpen] =
        useState<boolean>(false);
    const [deleteConversationId, setDeleteConversationId] =
        useState<string>("");
    const [deleteConversationName, setDeleteConversationName] =
        useState<string>("");
    const openDeleteDialog = useCallback((id: string, name: string) => {
        setDeleteConversationId(id);
        setDeleteConversationName(name);
        setDeleteDialogIsOpen(true);
    }, []);
    const closeDeleteDialog = useCallback(() => {
        setDeleteDialogIsOpen(false);
        setDeleteConversationId("");
        setDeleteConversationName("");
    }, []);

    return (
        <div
            className="flex-1 overflow-y-auto overflow-x-hidden px-3 bg-background"
            ref={scrollContainerRef}
            data-aipp-slot="chat-conversation-list"
        >
            <ul className="list-none p-0 m-0" data-aipp-slot="chat-conversation-list-items">
                {conversations.map((conversation) => (
                    <ConversationItem
                        key={conversation.id}
                        conversation={conversation}
                        isSelected={conversationId === conversation.id.toString()}
                        status={getItemStatus(conversation.id)}
                        onSelect={onSelectConversation}
                        onOpenTitleEdit={openTitleEditDialog}
                        onOpenDelete={openDeleteDialog}
                    />
                ))}
            </ul>

            {/* 加载指示器 */}
            {isLoading && conversations.length === 0 && (
                <div className="flex justify-center items-center py-8" data-aipp-slot="chat-conversation-list-loading">
                    <div className="text-sm text-muted-foreground">加载中...</div>
                </div>
            )}

            {isLoadingMore && (
                <div className="flex justify-center items-center py-4" data-aipp-slot="chat-conversation-list-loading-more">
                    <div className="text-sm text-muted-foreground">加载更多...</div>
                </div>
            )}

            {!hasMoreData && conversations.length > 0 && (
                <div className="flex justify-center items-center py-4" data-aipp-slot="chat-conversation-list-end">
                    <div className="text-xs text-muted-foreground">已加载全部对话</div>
                </div>
            )}

            <ConversationTitleEditDialog
                isOpen={titleEditDialogIsOpen}
                conversationId={editingConversationId}
                initialTitle={editingConversationTitle}
                onClose={closeTitleEditDialog}
            />

            <ConfirmDialog
                title={"确认删除对话"}
                confirmText={`确定要删除对话 "${deleteConversationName}" 吗？此操作无法撤销。`}
                onConfirm={() => {
                    deleteConversation(deleteConversationId, {
                        onSuccess: () => {
                            refreshList();
                            closeDeleteDialog();
                        },
                    });
                }}
                onCancel={closeDeleteDialog}
                isOpen={deleteDialogIsOpen}
            />
        </div>
    );
});

ConversationList.displayName = "ConversationList";

export default ConversationList;
