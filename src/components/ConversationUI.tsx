import { invoke } from "@tauri-apps/api/core";
import {
    Fragment,
    useCallback,
    useEffect,
    useMemo,
    useRef,
    useState,
    forwardRef,
    useImperativeHandle,
    useLayoutEffect,
    type ReactNode,
} from "react";

import {
    Conversation,
    ConversationCancelEvent,
    Message,
    QueuedConversationMessage,
    StreamEvent,
    ConversationWithMessages,
    GroupMergeEvent,
    MCPToolCallUpdateEvent,
    AcpConversationSessionState,
    AcpSessionConfigChoice,
    AcpSessionConfigOption,
    AgentActivityEvent,
} from "../data/Conversation";
import "katex/dist/katex.min.css";
import { listen, emit } from "@tauri-apps/api/event";
import FileDropArea from "./FileDropArea";
import useFileDropHandler from "../hooks/useFileDropHandler";
import InputArea, { InputAreaRef } from "./conversation/InputArea";
import MessageEditDialog from "./MessageEditDialog";
import ConversationTitleEditDialog from "./ConversationTitleEditDialog";
import { useMessageGroups } from "../hooks/useMessageGroups";
import useFileManagement from "@/hooks/useFileManagement";
import { useConversationEvents } from "@/hooks/useConversationEvents";
import { useAssistantListListener } from "@/hooks/useAssistantListListener";
import { AssistantListItem } from "@/data/Assistant";

// 导入新创建的 hooks
import { usePluginManagement } from "@/hooks/usePluginManagement";
import { useScrollManagement } from "@/hooks/useScrollManagement";
import { useTextSelection } from "@/hooks/useTextSelection";
import { useAssistantRuntime } from "@/hooks/useAssistantRuntime";
import { useMessageProcessing } from "@/hooks/useMessageProcessing";
import { useReasoningExpand } from "@/hooks/useReasoningExpand";
import { useConversationOperations } from "@/hooks/useConversationOperations";
import { useAntiLeakage } from "@/contexts/AntiLeakageContext";

// 导入新创建的组件
import ConversationHeader from "./conversation/ConversationHeader";
import ConversationContent, {
    type VirtualizedListEngine,
} from "./conversation/ConversationContent";
import ConversationTurnRail from "./conversation/ConversationTurnRail";
import { applyScrollHighlight } from "./conversation/scrollHighlight";
import { ToolErrorContinueProvider } from "./McpToolCall";
import IconButton from "./IconButton";
import { Badge } from "@/components/ui/badge";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Input } from "@/components/ui/input";
import { Separator } from "@/components/ui/separator";
import { Bot, Check, ChevronDown, LoaderCircle } from "lucide-react";
import { toast } from "sonner";
import { cn } from "@/utils/utils";

// 导入 Chat Sidebar 相关
import { ChatSidebar, type ContextItem, type TodoItem } from "./chat-sidebar";
import { useTodoList } from "@/hooks/useTodoList";
import { useArtifactExtractor } from "@/hooks/useArtifactExtractor";
import { useExplicitArtifacts } from "@/hooks/useExplicitArtifacts";
import { useContextList } from "@/hooks/useContextList";
import { mergeMessagesWithStreamingState } from "@/utils/streamingMessageState";
import { useFeatureConfig } from "@/hooks/feature/useFeatureConfig";
import { useDisplayConfig } from "@/hooks/useDisplayConfig";

// 暴露给外部的方法接口
export interface ConversationUIRef {
    focus: () => void;
    scrollToMessage: (messageId: number) => void;
    openStats: () => void;
    closeStats: () => void;
    openExport: () => void;
    closeExport: () => void;
    toggleSidebar: () => void;
    openSidebarWindow: () => void;
    openSettings: () => void;
}

export interface InlineInteractionItem {
    key: string;
    callId?: number | null;
    messageId?: number | null;
    content: ReactNode;
}

export interface PreviewFileContextSelection {
    callId: number;
    conversationId?: number;
    messageId?: number | null;
    requestId?: string | null;
}

interface AcpConfigSelectProps {
    option: AcpSessionConfigOption;
    disabled: boolean;
    onChange: (option: AcpSessionConfigOption, value: string) => void;
}

function formatAcpConfigChoice(choice: AcpSessionConfigChoice) {
    return choice.group_name ? `${choice.group_name} / ${choice.name}` : choice.name;
}

const ACP_CONFIG_VISIBLE_CHOICE_LIMIT = 80;

function AcpConfigSelect({ option, disabled, onChange }: AcpConfigSelectProps) {
    const [open, setOpen] = useState(false);
    const [query, setQuery] = useState("");
    const currentChoice = useMemo(
        () => option.options.find((choice) => choice.value === option.current_value),
        [option.current_value, option.options]
    );
    const currentLabel = currentChoice ? formatAcpConfigChoice(currentChoice) : option.current_value || "选择配置";
    const normalizedQuery = query.trim().toLowerCase();
    const matchingChoices = useMemo(() => {
        if (!normalizedQuery) {
            return option.options;
        }
        return option.options.filter((choice) => {
            const label = formatAcpConfigChoice(choice).toLowerCase();
            return label.includes(normalizedQuery) || choice.value.toLowerCase().includes(normalizedQuery);
        });
    }, [normalizedQuery, option.options]);
    const visibleChoices = matchingChoices.slice(0, ACP_CONFIG_VISIBLE_CHOICE_LIMIT);

    const handleOpenChange = (nextOpen: boolean) => {
        setOpen(nextOpen);
        if (!nextOpen) {
            setQuery("");
        }
    };

    const handleSelect = (value: string) => {
        onChange(option, value);
        setOpen(false);
        setQuery("");
    };

    return (
        <Popover open={open} onOpenChange={handleOpenChange}>
            <PopoverTrigger asChild>
                <button
                    type="button"
                    disabled={disabled}
                    className="border-input focus-visible:border-ring focus-visible:ring-ring/50 flex h-9 w-full items-center justify-between gap-2 rounded-md border bg-transparent px-3 py-2 text-left text-sm shadow-xs outline-none transition-[color,box-shadow] focus-visible:ring-[3px] disabled:cursor-not-allowed disabled:opacity-50"
                >
                    <span className="min-w-0 truncate">{currentLabel}</span>
                    <ChevronDown className="size-4 shrink-0 opacity-50" />
                </button>
            </PopoverTrigger>
            <PopoverContent align="start" className="w-[var(--radix-popover-trigger-width)] p-1">
                <Input
                    value={query}
                    onChange={(event) => setQuery(event.target.value)}
                    placeholder="搜索配置"
                    className="mb-1 h-8"
                />
                <div className="max-h-72 overflow-y-auto">
                    {visibleChoices.length === 0 ? (
                        <div className="px-2 py-6 text-center text-sm text-muted-foreground">无匹配项</div>
                    ) : (
                        visibleChoices.map((choice) => {
                            const selected = choice.value === option.current_value;
                            return (
                                <button
                                    key={`${option.id}:${choice.value}`}
                                    type="button"
                                    className={cn(
                                        "focus:bg-accent focus:text-accent-foreground flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-sm outline-none",
                                        selected && "bg-accent text-accent-foreground"
                                    )}
                                    onClick={() => handleSelect(choice.value)}
                                    title={formatAcpConfigChoice(choice)}
                                >
                                    <span className="min-w-0 flex-1 truncate">{formatAcpConfigChoice(choice)}</span>
                                    {selected ? <Check className="size-4 shrink-0" /> : null}
                                </button>
                            );
                        })
                    )}
                </div>
                {matchingChoices.length > visibleChoices.length ? (
                    <div className="border-t px-2 py-1.5 text-xs text-muted-foreground">
                        还有 {matchingChoices.length - visibleChoices.length} 项，输入关键词继续筛选
                    </div>
                ) : null}
            </PopoverContent>
        </Popover>
    );
}

interface ConversationUIProps {
    conversationId: string;
    onChangeConversationId: (conversationId: string) => void;
    pluginList: any[];
    isMobile?: boolean;
    hideHeader?: boolean;
    hideSidebar?: boolean;
    onConversationChange?: (conversation?: Conversation) => void;
    inlineInteractionItems?: InlineInteractionItem[];
    inlineInteractionVisible?: boolean;
    allowRename?: boolean;
    allowDelete?: boolean;
    headerExtraActions?: ReactNode;
    allowFeishuDebugResend?: boolean;
    virtualizeMessages?: boolean;
    virtualizedListEngine?: VirtualizedListEngine;
    windowLabel?: string;
    busySendBehavior?: "queue" | "interrupt";
    onPreviewFileContextClick?: (selection: PreviewFileContextSelection) => void;
}

// 左侧对话轮次导航条：从用户消息内容生成悬浮预览片段
const TURN_PREVIEW_MAX = 120;
function makeTurnPreview(content: string): string {
    const collapsed = content.replace(/\s+/g, " ").trim();
    if (collapsed.length <= TURN_PREVIEW_MAX) {
        return collapsed;
    }
    return collapsed.slice(0, TURN_PREVIEW_MAX) + "…";
}

const ConversationUI = forwardRef<ConversationUIRef, ConversationUIProps>(
    (
        {
            conversationId,
            onChangeConversationId,
            pluginList,
            isMobile = false,
            hideHeader = false,
            hideSidebar = false,
            onConversationChange,
            inlineInteractionItems,
            inlineInteractionVisible = false,
            allowRename = true,
            allowDelete = true,
            headerExtraActions,
            allowFeishuDebugResend = false,
            virtualizeMessages = false,
            virtualizedListEngine = "legacy",
            windowLabel = "chat_ui",
            busySendBehavior = "queue",
            onPreviewFileContextClick,
        },
        ref
    ) => {
        // ============= 基础状态管理 =============

        // 当前对话信息和助手列表
        const [conversation, setConversation] = useState<Conversation>();
        const [assistants, setAssistants] = useState<AssistantListItem[]>([]);
        const [selectedAssistant, setSelectedAssistant] = useState(-1);

        // 对话加载状态
        const [isLoadingShow, setIsLoadingShow] = useState(false);

        // ACP assistant working directory (resolved by backend)
        const [acpWorkingDirectory, setAcpWorkingDirectory] = useState<string | null>(null);
        const [acpMutationKey, setAcpMutationKey] = useState<string | null>(null);

        // 常规消息列表
        const [messages, setMessages] = useState<Array<Message>>([]);
        const [queuedMessages, setQueuedMessages] = useState<QueuedConversationMessage[]>([]);
        const streamingMessagesRef = useRef<Map<number, StreamEvent>>(new Map());
        const smartScrollRef = useRef<
            ((forceScroll?: boolean, behaviorOverride?: ScrollBehavior) => void) | null
        >(null);

        // AI响应状态管理
        const [aiIsResponsing, setAiIsResponsing] = useState<boolean>(false);

        // 输入相关状态
        const [inputText, setInputText] = useState("");
        const inputAreaRef = useRef<InputAreaRef>(null);
        // 加载请求标识，避免旧请求覆盖最新状态（StrictMode 双调用等场景）
        const loadRequestIdRef = useRef<number>(0);

        // ============= 使用新创建的 hooks =============

        // 插件管理
        const { assistantTypePluginMap, functionMap, setFunctionMapForMessage } = usePluginManagement(pluginList);

        // 文本选择
        const { selectedText } = useTextSelection();

        // 文件管理
        const { fileInfoList, clearFileInfoList, handleChooseFile, handleDeleteFile, handlePaste, handleDropFiles } =
            useFileManagement();

        // 文件拖拽
        const { isDragging, setIsDragging, dropRef } = useFileDropHandler(handleDropFiles);

        // Reasoning 展开状态
        const { reasoningExpandStates, toggleReasoningExpand } = useReasoningExpand();

        // 防泄露模式：获取重置函数
        const { resetReveal } = useAntiLeakage();
        const {
            getConfigValue,
            loadFeatureConfig,
            loading: featureConfigLoading,
        } = useFeatureConfig();
        const { config: displayConfig } = useDisplayConfig();
        const activeTheme = displayConfig?.theme ?? "default";
        const continueOnToolErrorEnabled = featureConfigLoading
            ? true
            : !["false", "0"].includes(
                getConfigValue("tool_error_continue", "enabled", "true").trim().toLowerCase(),
            );

        useEffect(() => {
            const unlisten = listen("feature_config_changed", () => {
                void loadFeatureConfig();
            });
            return () => {
                unlisten.then((f) => f());
            };
        }, [loadFeatureConfig]);

        // ============= Chat Sidebar Hooks =============

        // Todo list from built-in agent tool
        const { todos } = useTodoList({
            conversationId: conversationId ? parseInt(conversationId) : null,
        });

        // Sidebar expansion state and width
        const [, setSidebarExpanded] = useState(false);
        const [sidebarWidth, setSidebarWidth] = useState(0);
        const [sidebarToggleRequestVersion, setSidebarToggleRequestVersion] = useState(0);

        // Sidebar window state - when true, hide the inline sidebar
        const [sidebarWindowOpen, setSidebarWindowOpen] = useState(false);

        // Dialog states for shortcut triggering
        const [statsDialogOpen, setStatsDialogOpen] = useState(false);
        const [exportDialogOpen, setExportDialogOpen] = useState(false);

        const handleSidebarExpandChange = useCallback((isExpanded: boolean, width: number) => {
            setSidebarExpanded(isExpanded);
            setSidebarWidth(width);
        }, []);

        // Reset sidebar width when conversation changes or sidebar becomes invisible
        useEffect(() => {
            if (!conversationId || isMobile) {
                setSidebarWidth(0);
            }
        }, [conversationId, isMobile]);

        // ============= 事件处理逻辑 =============

        const handleMessageAdd = useCallback(
            (messageAddData: any) => {
                // 设置函数映射
                setFunctionMapForMessage(messageAddData.message_id);

                // 发送新消息时，重置防泄露模式的临时显示状态
                resetReveal();

                // 重新获取对话消息，以确保获得完整的消息数据（包括generation_group_id等）
                invoke<ConversationWithMessages>("get_conversation_with_messages", {
                    conversationId: +conversationId,
                })
                    .then((updatedConversation) => {
                        setMessages((prevMessages) =>
                            mergeMessagesWithStreamingState(updatedConversation.messages, {
                                conversationId: updatedConversation.conversation.id,
                                currentMessages: prevMessages,
                                streamingSnapshot: streamingMessagesRef.current,
                            })
                        );
                    })
                    .catch((error) => {
                        console.error("Failed to reload conversation after message_add:", error);

                        // 降级处理：仍然添加基本的消息信息
                        const newMessage: Message = {
                            id: messageAddData.message_id,
                            conversation_id: +conversationId,
                            message_type: messageAddData.message_type,
                            content: "", // 初始内容为空，会通过后续的message_update事件更新
                            llm_model_id: null,
                            created_time: new Date(),
                            start_time: new Date(),
                            finish_time: null,
                            token_count: 0,
                            input_token_count: 0,
                            output_token_count: 0,
                            generation_group_id: null, // 这些字段会在数据库查询时填充
                            parent_group_id: null,
                            regenerate: null,
                        };

                        setMessages((prevMessages) => [...prevMessages, newMessage]);
                    });
            },
            [conversationId, resetReveal, setFunctionMapForMessage]
        );

        const handleGroupMerge = useCallback((groupMergeData: GroupMergeEvent) => {
            // 设置组合并关系
            setGroupMergeMap((prev) => {
                const newMap = new Map(prev);
                newMap.set(groupMergeData.new_group_id, groupMergeData.original_group_id);
                return newMap;
            });
        }, []);

        const handleAiResponseStart = useCallback(() => {
            setAiIsResponsing(true);
        }, [setAiIsResponsing]);

        const handleAiResponseComplete = useCallback(() => {
            setAiIsResponsing(false);
        }, []);

        const handleError = useCallback((errorMessage: string) => {
            console.error("Stream error from conversation events:", errorMessage);
            // 确保AI响应状态被重置
            setAiIsResponsing(false);
            // 不再显示toast，错误信息将在对话框中显示
        }, []);

        const handleMCPToolCallUpdate = useCallback((mcpUpdateData: MCPToolCallUpdateEvent) => {
            console.log("ConversationUI received MCP update:", mcpUpdateData);
            // MCP状态更新已经在useConversationEvents中处理，这里可以添加额外的逻辑
        }, []);

        const upsertQueuedMessage = useCallback((queued: QueuedConversationMessage) => {
            setQueuedMessages((current) => {
                const withoutCurrent = current.filter((item) => item.id !== queued.id);
                return [...withoutCurrent, queued].sort((left, right) => left.id - right.id);
            });
        }, []);

        const handleQueuedMessageRemove = useCallback(
            (payload: { id: number; conversation_id: number }) => {
                setQueuedMessages((current) =>
                    current.filter((item) => item.id !== payload.id)
                );
            },
            []
        );

        // ============= 消息处理逻辑 =============

        // 处理消息完成时的状态更新，确保消息在streamingMessages清理后仍能显示
        const handleMessageCompletion = useCallback(
            (streamEvent: StreamEvent) => {
                const effectiveConversationId = conversation?.id ?? Number(conversationId || 0);
                setMessages((prevMessages) =>
                    mergeMessagesWithStreamingState(prevMessages, {
                        conversationId: effectiveConversationId,
                        currentMessages: prevMessages,
                        streamingSnapshot: new Map([[streamEvent.message_id, streamEvent]]),
                        finalizeStreaming: true,
                    })
                );
            },
            [conversation?.id, conversationId]
        );

        // 使用 useMemo 稳定 options 对象，避免频繁触发 useConversationEvents 内部的 useEffect
        const conversationEventsOptions = useMemo(() => {
            const handleMessageUpdate = (streamEvent: StreamEvent) => {
                // 处理插件兼容性 - 现在从 ref 中获取最新的 functionMap
                // 这里需要从 useConversationEvents 内部处理，所以暂时移除
                // const streamMessageListener = functionMap.get(
                //     streamEvent.message_id,
                // )?.onStreamMessageListener;
                // if (streamMessageListener) {
                //     streamMessageListener(
                //         streamEvent.content,
                //         { conversation_id: +conversationId, request_prompt_result_with_context: "" },
                //         setAiIsResponsing,
                //     );
                // }

                if (streamEvent.is_done) {
                    // 在清理streamingMessages之前，先将消息添加到messages状态
                    handleMessageCompletion(streamEvent);
                }

                // 每次消息更新时手动触发滚动
                setTimeout(() => smartScrollRef.current?.(), 0);
            };

            return {
                conversationId: conversationId,
                onMessageAdd: handleMessageAdd,
                onMessageUpdate: handleMessageUpdate,
                onConversationCancel: (
                    cancelData: ConversationCancelEvent,
                    streamingSnapshot: ReadonlyMap<number, StreamEvent>
                ) => {
                    if (streamingSnapshot.size === 0) {
                        return;
                    }

                    const effectiveConversationId =
                        conversation?.id ?? Number(conversationId || 0);
                    setMessages((prevMessages) =>
                        mergeMessagesWithStreamingState(prevMessages, {
                            conversationId: effectiveConversationId,
                            currentMessages: prevMessages,
                            streamingSnapshot,
                            finalizeStreaming: true,
                            finalizedAt: new Date(cancelData.cancelled_at),
                        })
                    );
                },
                onGroupMerge: handleGroupMerge,
                onMCPToolCallUpdate: handleMCPToolCallUpdate,
                onAiResponseStart: handleAiResponseStart,
                onAiResponseComplete: handleAiResponseComplete,
                onError: handleError,
                onQueuedMessageAdd: upsertQueuedMessage,
                onQueuedMessageUpdate: upsertQueuedMessage,
                onQueuedMessageRemove: handleQueuedMessageRemove,
            };
        }, [
            conversationId,
            handleMessageAdd,
            handleGroupMerge,
            handleMCPToolCallUpdate,
            handleAiResponseStart,
            handleAiResponseComplete,
            handleError,
            handleMessageCompletion,
            upsertQueuedMessage,
            handleQueuedMessageRemove,
            conversation?.id,
            // 移除 functionMap 依赖，改为在回调内部访问
        ]);

        // 使用共享的消息事件处理 hook
        const {
            streamingMessages,
            shiningMessageIds,
            setShiningMessageIds,
            setManualShineMessage,
            mcpToolCallStates,
            agentActivities,
            shiningMcpCallId,
            runtimeState,
            updateShiningMessages,
            updateFunctionMap,
            clearStreamingMessages,
            clearShiningMessages,
            setPendingUserMessage,
            acpSessionState,
            applyAcpSessionState,
        } = useConversationEvents(conversationEventsOptions);

        const acpLoadUnsupportedNoticeRef = useRef<string | null>(null);
        const acpRestoreNoticeRef = useRef<string | null>(null);
        const acpAutoConnectKeyRef = useRef<string | null>(null);
        const acpConnectionErrorNoticeRef = useRef<string | null>(null);

        useEffect(() => {
            const conversationIdNum = Number(conversationId);
            if (!conversationIdNum || Number.isNaN(conversationIdNum)) {
                setQueuedMessages([]);
                return;
            }

            let cancelled = false;
            invoke<QueuedConversationMessage[]>("list_queued_conversation_messages", {
                conversationId: conversationIdNum,
            })
                .then((items) => {
                    if (cancelled) {
                        return;
                    }
                    setQueuedMessages(items.sort((left, right) => left.id - right.id));
                })
                .catch((error) => {
                    if (cancelled) {
                        return;
                    }
                    console.warn("Failed to load queued conversation messages:", error);
                    setQueuedMessages([]);
                });

            return () => {
                cancelled = true;
            };
        }, [conversationId]);

        const showAcpConnectionError = useCallback(
            (assistantId: number, error: unknown) => {
                const errorMessage =
                    error instanceof Error
                        ? error.message
                        : typeof error === "string"
                            ? error
                            : JSON.stringify(error) || "未知错误";
                const noticeKey = `${conversationId}:${assistantId}:${errorMessage}`;
                if (acpConnectionErrorNoticeRef.current === noticeKey) {
                    return;
                }

                acpConnectionErrorNoticeRef.current = noticeKey;
                toast.error("ACP 会话启动失败", {
                    description: errorMessage,
                    position: "bottom-right",
                });
            },
            [conversationId]
        );

        useEffect(() => {
            streamingMessagesRef.current = streamingMessages;
        }, [streamingMessages]);

        useEffect(() => {
            const canRestoreAcpSession =
                acpSessionState?.load_session_supported ||
                acpSessionState?.session_resume_supported;
            if (!acpSessionState?.session_id || canRestoreAcpSession) {
                return;
            }

            const noticeKey = `${conversationId}:${acpSessionState.session_id}`;
            if (acpLoadUnsupportedNoticeRef.current === noticeKey) {
                return;
            }

            acpLoadUnsupportedNoticeRef.current = noticeKey;
            toast.info("该 Agent 不支持恢复历史 ACP 会话", {
                description: "AIPP 会使用本地对话上下文继续当前请求。",
            });
        }, [
            acpSessionState?.load_session_supported,
            acpSessionState?.session_resume_supported,
            acpSessionState?.session_id,
            conversationId,
        ]);

        useEffect(() => {
            const method = acpSessionState?.restored_session_method;
            if (!acpSessionState?.session_id || !method) {
                return;
            }

            const noticeKey = `${conversationId}:${acpSessionState.session_id}:${method}`;
            if (acpRestoreNoticeRef.current === noticeKey) {
                return;
            }

            acpRestoreNoticeRef.current = noticeKey;
            const methodLabel = method === "resume" ? "session/resume" : "session/load";
            const description =
                method === "resume"
                    ? "AIPP 保留本地对话展示，Agent 仅恢复内部上下文。"
                    : "AIPP 已抑制历史回放，避免重复写入当前对话。";
            toast.success(`已通过 ${methodLabel} 恢复 ACP 会话`, {
                description,
            });
        }, [
            acpSessionState?.restored_session_method,
            acpSessionState?.session_id,
            conversationId,
        ]);

        const effectiveAiIsResponsing = useMemo(() => {
            if (runtimeState && runtimeState.conversation_id === Number(conversationId || 0)) {
                return runtimeState.is_running;
            }
            return aiIsResponsing;
        }, [
            aiIsResponsing,
            runtimeState?.conversation_id,
            runtimeState?.is_running,
            conversationId,
        ]);
        const attachedFileCount = Array.isArray(fileInfoList) ? fileInfoList.length : 0;

        const queuedDisplayMessages = useMemo<Message[]>(() => {
            return queuedMessages.map((queued) => {
                const queueKind = queued.queue_kind === "interrupt" ? "interrupt" : "normal";
                const createdTime = queued.created_time ? new Date(queued.created_time) : new Date();
                return {
                    id: -queued.id,
                    conversation_id: queued.conversation_id,
                    message_type: "user",
                    content: queued.prompt,
                    llm_model_id: null,
                    created_time: createdTime,
                    start_time: null,
                    finish_time: null,
                    token_count: 0,
                    input_token_count: 0,
                    output_token_count: 0,
                    generation_group_id: null,
                    parent_group_id: null,
                    parent_id: null,
                    regenerate: null,
                    attachment_list: [],
                    metadata_json: JSON.stringify({
                        queue_status: "queued",
                        queue_kind: queueKind,
                        queue_id: queued.id,
                    }),
                };
            });
        }, [queuedMessages]);

        const messagesWithQueued = useMemo(
            () => [...messages, ...queuedDisplayMessages],
            [messages, queuedDisplayMessages]
        );

        // 当 functionMap 变化时更新事件处理器
        useEffect(() => {
            updateFunctionMap(functionMap);
        }, [functionMap, updateFunctionMap]);

        // 消息处理 - 首先需要获取 groupMergeMap
        const [groupMergeMap, setGroupMergeMap] = useState<Map<string, string>>(new Map());

        // 第一步：消息处理 - 获取合并的消息用于分组
        const { combinedMessagesForGrouping } = useMessageProcessing({
            messages: messagesWithQueued,
            streamingMessages,
            conversation,
            generationGroups: new Map(), // 第一步只需要合并消息用于分组
            groupRootMessageIds: new Map(),
            getMessageVersionInfo: () => ({ shouldShow: true }),
        });

        // 第二步：使用合并后的消息进行分组计算
        const messageGroupsData = useMessageGroups({
            allDisplayMessages: combinedMessagesForGrouping,
            groupMergeMap,
        });

        // 第三步：基于分组信息与选择的版本，计算最终需要展示的消息列表
        const { allDisplayMessages } = useMessageProcessing({
            messages: messagesWithQueued,
            streamingMessages,
            conversation,
            generationGroups: messageGroupsData.generationGroups,
            groupRootMessageIds: messageGroupsData.groupRootMessageIds,
            getMessageVersionInfo: messageGroupsData.getMessageVersionInfo,
        });
        const displayAgentActivities = useMemo(() => {
            const combined = new Map<string, AgentActivityEvent>();
            for (const message of allDisplayMessages) {
                if (!message.metadata_json) continue;
                try {
                    const metadata = JSON.parse(message.metadata_json) as {
                        agent_activities?: AgentActivityEvent[];
                    };
                    for (const activity of metadata.agent_activities ?? []) {
                        if (!activity?.item_id || !activity.agent_kind) continue;
                        const key = `${activity.agent_kind}:${activity.session_id ?? ""}:${activity.item_id}`;
                        const existing = combined.get(key);
                        if (!existing || existing.sequence < activity.sequence) {
                            combined.set(key, activity);
                        }
                    }
                } catch {
                    // Other message metadata is allowed to be non-Agent JSON.
                }
            }
            for (const [key, activity] of agentActivities) {
                const existing = combined.get(key);
                if (!existing || existing.sequence < activity.sequence) {
                    combined.set(key, activity);
                }
            }
            return combined;
        }, [agentActivities, allDisplayMessages]);
        // 滚动管理 - 移除依赖项，改为手动调用
        const {
            messagesEndRef,
            scrollContainerRef,
            handleScroll,
            handleUserScrollIntent,
            syncScrollState,
            smartScroll,
            scrollToUserMessage,
            scrollToBottomStable,
        } = useScrollManagement({
            disableTailObservation: virtualizeMessages,
        });
        smartScrollRef.current = smartScroll;
        const [pendingScrollMessageId, setPendingScrollMessageId] = useState<number | null>(null);

        // ============= Chat Sidebar 数据提取 =============

        // Artifacts from messages (code blocks)
        const { artifacts: inferredArtifacts } = useArtifactExtractor({
            messages: allDisplayMessages,
        });
        const { artifacts: explicitArtifacts } = useExplicitArtifacts({
            conversationId,
            mcpToolCallStates,
        });
        const artifacts = explicitArtifacts.length > 0 ? explicitArtifacts : inferredArtifacts;

        // Context items (user files + MCP tool calls + message attachments)
        const { contextItems } = useContextList({
            conversationId,
            userFiles: fileInfoList,
            mcpToolCallStates,
            messages,
            acpWorkingDirectory,
        });

        const handlePreviewFileContextClick = useCallback((item: ContextItem) => {
            if (!item.previewFileData) {
                return;
            }
            onPreviewFileContextClick?.({
                ...item.previewFileData,
                conversationId: item.previewFileData.conversationId ?? Number(conversationId),
            });
        }, [conversationId, onPreviewFileContextClick]);

        // ============= Sidebar Window 事件处理 =============

        // Inline-mode focus target (used only when the sidebar window is NOT open)
        const [focusedContextId, setFocusedContextId] = useState<string | null>(null);

        // Listen for sidebar window open/close events
        useEffect(() => {
            const unlistenOpened = listen("sidebar-window-opened", () => {
                setSidebarWindowOpen(true);
                setSidebarWidth(0); // Reset sidebar width when window opens
                // Send data immediately when window opens
                emit("sidebar-data-sync", {
                    todos,
                    artifacts,
                    contextItems,
                    conversationId,
                });
            });

            const unlistenClosed = listen("sidebar-window-closed", () => {
                setSidebarWindowOpen(false);
            });

            // Listen for sidebar window ready event and send data
            const unlistenReady = listen("sidebar-window-ready", () => {
                emit("sidebar-data-sync", {
                    todos,
                    artifacts,
                    contextItems,
                    conversationId,
                });
            });

            return () => {
                unlistenOpened.then((f) => f());
                unlistenClosed.then((f) => f());
                unlistenReady.then((f) => f());
            };
        }, [todos, artifacts, contextItems, conversationId]);

        // Listen for focus-context requests from inline MCP tool cards. Only react
        // when the separate sidebar window is NOT open — otherwise the sidebar
        // window handles its own focus/scroll/highlight.
        useEffect(() => {
            const unlisten = listen<{ id: string }>("sidebar-focus-context", (event) => {
                if (sidebarWindowOpen) return;
                setFocusedContextId(event.payload.id);
            });
            return () => {
                unlisten.then((f) => f());
            };
        }, [sidebarWindowOpen]);

        // Auto-clear inline highlight after 1s to match the sidebar window behavior
        useEffect(() => {
            if (!focusedContextId) return;
            const timer = setTimeout(() => setFocusedContextId(null), 1000);
            return () => clearTimeout(timer);
        }, [focusedContextId]);

        // Sync sidebar data to window when data changes (if window is open)
        useEffect(() => {
            if (sidebarWindowOpen) {
                emit("sidebar-data-sync", {
                    todos,
                    artifacts,
                    contextItems,
                    conversationId,
                });
            }
        }, [sidebarWindowOpen, todos, artifacts, contextItems, conversationId]);

        // Handle opening the sidebar window
        const handleOpenSidebarWindow = useCallback(() => {
            invoke("open_sidebar_window");
        }, []);

        // 助手运行时API
        const { assistantRunApi } = useAssistantRuntime({
            conversation,
            selectedAssistant,
            inputText,
            fileInfoList: fileInfoList || undefined,
            setMessages,
            onChangeConversationId,
            smartScroll,
            updateShiningMessages,
            setAiIsResponsing,
        });

        const handleQueuedMessagePromote = useCallback((queueId: number) => {
            void invoke<QueuedConversationMessage>("promote_queued_conversation_message", {
                queueId,
            }).catch((error) => {
                toast.error("提升打断消息失败", {
                    description: String(error),
                    position: "bottom-right",
                });
            });
        }, []);

        // 对话操作
        const {
            handleDeleteConversationSuccess,
            handleMessageRegenerate,
            handleMessageEdit,
            handleMessageFork,
            handleEditSave,
            handleEditSaveAndRegenerate,
            handleSend,
            handleArtifact,
            editDialogIsOpen,
            editingMessage,
            closeEditDialog,
            titleEditDialogIsOpen,
            openTitleEditDialog,
            closeTitleEditDialog,
        } = useConversationOperations({
            conversation,
            selectedAssistant,
            assistants,
            setMessages,
            inputText,
            setInputText,
            fileInfoList: fileInfoList || undefined,
            clearFileInfoList,
            aiIsResponsing: effectiveAiIsResponsing,
            setAiIsResponsing,
            onChangeConversationId,
            setShiningMessageIds,
            setManualShineMessage,
            updateShiningMessages,
            clearShiningMessages,
            assistantTypePluginMap,
            assistantRunApi,
            busySendBehavior,
        });

        // ============= 初始化和生命周期逻辑 =============

        // 暴露给外部的方法
        useImperativeHandle(
            ref,
            () => ({
                focus: () => {
                    inputAreaRef.current?.focus();
                },
                scrollToMessage: (messageId: number) => {
                    setPendingScrollMessageId(messageId);
                },
                openStats: () => {
                    if (conversationId) setStatsDialogOpen(true);
                },
                closeStats: () => {
                    setStatsDialogOpen(false);
                },
                openExport: () => {
                    if (conversationId) setExportDialogOpen(true);
                },
                closeExport: () => {
                    setExportDialogOpen(false);
                },
                toggleSidebar: () => {
                    setSidebarToggleRequestVersion((current) => current + 1);
                },
                openSidebarWindow: () => {
                    handleOpenSidebarWindow();
                },
                openSettings: () => {
                    invoke("open_config_window");
                },
            }),
            [conversationId, handleOpenSidebarWindow]
        );

        // 左侧对话轮次导航条：从当前展示的消息中抽取用户提问轮次
        const userTurns = useMemo(
            () =>
                allDisplayMessages
                    .filter(
                        (m) =>
                            m.message_type === "user" &&
                            !m.content?.startsWith("Tool execution results:\n") &&
                            (m.content?.trim().length ?? 0) > 0,
                    )
                    .map((m) => ({
                        id: m.id,
                        preview: makeTurnPreview(m.content),
                    })),
            [allDisplayMessages],
        );

        // 智能聚焦逻辑 - 无延迟版本
        useLayoutEffect(() => {
            // 只在 InputArea 存在且不在加载状态时聚焦
            if (inputAreaRef.current && !isLoadingShow) {
                inputAreaRef.current.focus();
            }
        }, [conversationId, isLoadingShow]); // 监听对话ID和加载状态变化

        // 通知父组件当前对话信息变化，用于移动端标题展示
        useEffect(() => {
            if (onConversationChange) {
                onConversationChange(conversation);
            }
        }, [conversation, onConversationChange]);

        // 对话加载和管理逻辑
        // 注意：为避免 React StrictMode 下的双调用导致“取消”标记错误触发，使用 requestId 跳过过期请求
        useEffect(() => {
            // 仅依赖 conversationId，保持函数引用稳定
            if (!conversationId) {
                // 无对话 ID时，清理状态并加载助手列表
                setMessages([]);
                setConversation(undefined);
                // 清理流式消息和闪烁状态
                clearStreamingMessages();
                clearShiningMessages();

                invoke<Array<AssistantListItem>>("get_assistants").then((assistantList) => {
                    setAssistants(assistantList);
                    if (assistantList.length > 0) {
                        setSelectedAssistant(assistantList[0].id);
                    }
                });
                return;
            }

            // 使用递增的 requestId 避免旧请求覆盖最新状态
            const requestId = (loadRequestIdRef.current || 0) + 1;
            loadRequestIdRef.current = requestId;

            // 加载指定对话的消息和信息
            setIsLoadingShow(true);
            console.log(`[DEBUG] Starting to load conversation: ${conversationId}, requestId: ${requestId}`);

            // 在切换对话时立即清理所有与前一个对话相关的状态
            setGroupMergeMap(new Map()); // 切换对话时清理组合并状态
            clearStreamingMessages(); // 清理流式消息
            clearShiningMessages(); // 清理闪烁状态
            // 立即清空当前消息与会话，避免先渲染旧数据再渲染新数据导致的双次渲染
            setMessages([]);
            setConversation(undefined);

            // 切换对话时，重置防泄露模式的临时显示状态
            resetReveal();

            console.log(`[PERF-FRONTEND] conversationId change : ${conversationId}`);
            const frontendStartTime = performance.now();

            invoke<ConversationWithMessages>("get_conversation_with_messages", {
                conversationId: +conversationId,
            })
                .then((res: ConversationWithMessages) => {
                    // 仅处理最新请求
                    if (loadRequestIdRef.current !== requestId) {
                        console.log(`[DEBUG] Skip stale response for conversationId: ${conversationId}, requestId: ${requestId}`);
                        return;
                    }

                    const backendDuration = performance.now() - frontendStartTime;
                    console.log(`[PERF-FRONTEND] 后端返回数据耗时: ${backendDuration.toFixed(2)}ms, 消息数: ${res.messages.length}`);

                    const setStateStartTime = performance.now();
                    setMessages(res.messages);
                    setConversation(res.conversation);
                    setIsLoadingShow(false); // 这里会触发 useLayoutEffect 中的聚焦

                    if (res.messages.length === 2) {
                        if (res.messages[0].message_type === "system" && res.messages[1].message_type === "user") {
                            setPendingUserMessage(res.messages[1].id);
                        }
                    }

                    const setStateDuration = performance.now() - setStateStartTime;
                    console.log(`[PERF-FRONTEND] 设置状态耗时: ${setStateDuration.toFixed(2)}ms`);
                })
                .catch((error) => {
                    if (loadRequestIdRef.current !== requestId) {
                        console.log(`[DEBUG] Skip stale error for conversationId: ${conversationId}, requestId: ${requestId}`);
                        return;
                    }
                    console.error("Failed to load conversation:", error);
                    setIsLoadingShow(false);
                });

            // 不使用清理函数的取消标记，依赖 requestId 判定最新请求
        }, [conversationId]);

        // 监听对话标题变化
        useEffect(() => {
            const unsubscribe = listen("title_change", (event) => {
                const [conversationId, title] = event.payload as [number, string];

                if (conversation && conversation.id === conversationId) {
                    const newConversation = { ...conversation, name: title };
                    setConversation(newConversation);
                }
            });

            return () => {
                if (unsubscribe) {
                    unsubscribe.then((f) => f());
                }
            };
        }, [conversation]);

        // 监听助手列表变化
        useAssistantListListener({
            onAssistantListChanged: useCallback(
                (assistantList: AssistantListItem[]) => {
                    setAssistants(assistantList);
                    // 如果当前选中的助手不在新列表中，选择第一个助手
                    if (
                        assistantList.length > 0 &&
                        !assistantList.some((assistant) => assistant.id === selectedAssistant)
                    ) {
                        setSelectedAssistant(assistantList[0].id);
                    }
                },
                [selectedAssistant]
            ),
        });

        const activeAssistantId = conversation?.assistant_id ?? selectedAssistant;
        const isAcpAssistant = useMemo(() => {
            if (!activeAssistantId) {
                return false;
            }
            return assistants.some(
                (assistant) => assistant.id === activeAssistantId && assistant.assistant_type === 4
            );
        }, [activeAssistantId, assistants]);

        // Fetch ACP working directory for current assistant
        useEffect(() => {
            const assistantId = activeAssistantId;
            if (!assistantId || !isAcpAssistant) {
                setAcpWorkingDirectory(null);
                applyAcpSessionState(null);
                return;
            }

            invoke<string>("get_acp_working_directory", { assistantId })
                .then((workingDirectory) => {
                    setAcpWorkingDirectory(workingDirectory);
                    const conversationIdNum = Number(conversationId);
                    if (!conversationIdNum || Number.isNaN(conversationIdNum)) {
                        return;
                    }
                    const connectKey = `${conversationIdNum}:${assistantId}`;
                    if (acpAutoConnectKeyRef.current === connectKey) {
                        return;
                    }
                    acpAutoConnectKeyRef.current = connectKey;
                    void invoke<AcpConversationSessionState | null>("ensure_acp_session_connected", {
                        conversationId: conversationIdNum,
                        assistantId,
                    })
                        .then((state) => {
                            if (acpAutoConnectKeyRef.current !== connectKey) {
                                return;
                            }
                            acpAutoConnectKeyRef.current = null;
                            applyAcpSessionState(state);
                        })
                        .catch((error) => {
                            if (acpAutoConnectKeyRef.current === connectKey) {
                                acpAutoConnectKeyRef.current = null;
                            }
                            console.warn("[ACP] Auto connect failed", error);
                            showAcpConnectionError(assistantId, error);
                        });
                })
                .catch((error) => {
                    setAcpWorkingDirectory(null);
                    showAcpConnectionError(assistantId, error);
                });
        }, [
            activeAssistantId,
            isAcpAssistant,
            conversationId,
            applyAcpSessionState,
            showAcpConnectionError,
        ]);

        const handleAcpConfigChange = useCallback(
            async (option: AcpSessionConfigOption, value: string) => {
                const conversationIdNum = Number(conversationId);
                if (!conversationIdNum || Number.isNaN(conversationIdNum)) {
                    return;
                }

                setAcpMutationKey(`config:${option.id}`);
                try {
                    await invoke("set_acp_session_config_option", {
                        conversationId: conversationIdNum,
                        configId: option.id,
                        value,
                    });
                } catch (error) {
                    toast.error("更新 ACP 配置失败", {
                        description: String(error),
                        position: "bottom-right",
                    });
                } finally {
                    setAcpMutationKey(null);
                }
            },
            [conversationId]
        );

        const pluginHeaderActions = useMemo(() => {
            const actionContext = {
                conversationId: conversation?.id ?? null,
                assistantId: conversation?.assistant_id ?? null,
                conversationName: conversation?.name ?? "",
                assistantName: conversation?.assistant_name ?? "",
            };
            const actionEntries = pluginList
                .flatMap((plugin) =>
                    (plugin.contributions?.actions ?? []).map((action: {
                        id: string;
                        location: string;
                        order?: number | null;
                    }) => ({
                        plugin,
                        action,
                    }))
                )
                .filter(({ action }) => action.location === "conversation.title-actions")
                .sort((left, right) => {
                    const leftOrder = left.action.order ?? 100;
                    const rightOrder = right.action.order ?? 100;
                    if (leftOrder !== rightOrder) {
                        return leftOrder - rightOrder;
                    }
                    return String(left.plugin.code ?? "").localeCompare(String(right.plugin.code ?? ""));
                });

            if (actionEntries.length === 0) {
                return null;
            }

            return actionEntries.map(({ plugin, action }) => {
                const instance = plugin.instance as
                    | {
                        renderAction?: (actionId: string, context?: Record<string, unknown>) => React.ReactNode;
                    }
                    | null;
                if (typeof instance?.renderAction !== "function") {
                    return null;
                }
                try {
                    const rendered = instance.renderAction(action.id, actionContext);
                    if (!rendered) {
                        return null;
                    }
                    return <Fragment key={`${plugin.code}:${action.id}:${conversation?.id ?? "none"}`}>{rendered}</Fragment>;
                } catch (error) {
                    console.error(
                        `[ConversationUI] Failed to render plugin action '${action.id}' from '${plugin.code}':`,
                        error
                    );
                    return null;
                }
            });
        }, [conversation, pluginList]);

        const combinedHeaderActions = useMemo(() => {
            if (!pluginHeaderActions && !headerExtraActions) {
                return null;
            }
            return (
                <>
                    {pluginHeaderActions}
                    {headerExtraActions}
                </>
            );
        }, [headerExtraActions, pluginHeaderActions]);

        const renderPluginMessageActions = useCallback((message: Message) => {
            const actionEntries = pluginList
                .flatMap((plugin) =>
                    (plugin.contributions?.actions ?? []).map((action: {
                        id: string;
                        location: string;
                        order?: number | null;
                    }) => ({ plugin, action }))
                )
                .filter(({ action }) => action.location === "conversation.message-actions")
                .sort((left, right) => (left.action.order ?? 100) - (right.action.order ?? 100));

            if (actionEntries.length === 0) {
                return null;
            }

            const actionContext = {
                conversationId: message.conversation_id,
                messageId: message.id,
                messageType: message.message_type,
                messageContent: message.content,
            };

            return actionEntries.map(({ plugin, action }) => {
                const instance = plugin.instance as
                    | { renderAction?: (actionId: string, context?: Record<string, unknown>) => React.ReactNode }
                    | null;
                if (typeof instance?.renderAction !== "function") {
                    return null;
                }
                try {
                    const rendered = instance.renderAction(action.id, actionContext);
                    return rendered ? (
                        <Fragment key={`${plugin.code}:${action.id}:${message.id}`}>{rendered}</Fragment>
                    ) : null;
                } catch (error) {
                    console.error(
                        `[ConversationUI] Failed to render message plugin action '${action.id}' from '${plugin.code}':`,
                        error
                    );
                    return null;
                }
            });
        }, [pluginList]);

        const sendButtonSlotContext = useMemo(
            () => ({
                conversationId: conversation?.id ?? null,
                assistantId: conversation?.assistant_id ?? null,
                conversationName: conversation?.name ?? "",
                assistantName: conversation?.assistant_name ?? "",
                isResponding: effectiveAiIsResponsing,
                inputText,
                fileCount: attachedFileCount,
                placement: "bottom",
                isMobile,
                windowLabel,
                activeTheme,
            }),
            [
                activeTheme,
                attachedFileCount,
                conversation,
                effectiveAiIsResponsing,
                inputText,
                isMobile,
                windowLabel,
            ]
        );

        const renderSendButtonSlot = useCallback((location: string) => {
            const slotEntries = pluginList
                .flatMap((plugin) =>
                    (plugin.contributions?.slots ?? []).map((slot: {
                        id: string;
                        location: string;
                        order?: number | null;
                    }) => ({
                        plugin,
                        slot,
                    }))
                )
                .filter(({ slot }) => slot.location === location)
                .sort((left, right) => {
                    const leftOrder = left.slot.order ?? 100;
                    const rightOrder = right.slot.order ?? 100;
                    if (leftOrder !== rightOrder) {
                        return leftOrder - rightOrder;
                    }
                    return String(left.plugin.code ?? "").localeCompare(String(right.plugin.code ?? ""));
                });

            for (const { plugin, slot } of slotEntries) {
                const instance = plugin.instance as
                    | {
                        renderSlot?: (slotId: string, context?: Record<string, unknown>) => ReactNode;
                    }
                    | null;
                if (typeof instance?.renderSlot !== "function") {
                    continue;
                }
                try {
                    const rendered = instance.renderSlot(slot.id, sendButtonSlotContext);
                    if (rendered) {
                        return rendered;
                    }
                } catch (error) {
                    console.error(
                        `[ConversationUI] Failed to render plugin slot '${slot.id}' from '${plugin.code}':`,
                        error
                    );
                }
            }
            return null;
        }, [pluginList, sendButtonSlotContext]);

        const sendButtonVisualSlot = useMemo(
            () => renderSendButtonSlot("chat.input.send-button-visual"),
            [renderSendButtonSlot]
        );

        const sendButtonIconSlot = useMemo(
            () => renderSendButtonSlot("chat.input.send-button-icon"),
            [renderSendButtonSlot]
        );

        const acpHeaderActions = useMemo(() => {
            if (!isAcpAssistant) {
                return combinedHeaderActions;
            }

            const modelOption = acpSessionState?.config_options.find((option) => option.category === "model");
            const modeOption = acpSessionState?.config_options.find((option) => option.category === "mode");
            const thoughtOption = acpSessionState?.config_options.find((option) => option.category === "thought_level");
            const primaryOptionIds = new Set(
                [modelOption?.id, modeOption?.id, thoughtOption?.id].filter(Boolean)
            );
            const otherOptions = acpSessionState?.config_options.filter(
                (option) => !primaryOptionIds.has(option.id)
            ) ?? [];
            const updatedAtText = acpSessionState?.updated_at
                ? new Date(acpSessionState.updated_at).toLocaleString()
                : null;
            const configOptionCount = acpSessionState?.config_options.length ?? 0;
            const renderAcpConfigSelect = (option: AcpSessionConfigOption) => (
                <div key={option.id} className="space-y-1.5">
                    <div className="flex items-center gap-2">
                        <span className="text-xs font-medium">{option.name}</span>
                        {option.category ? (
                            <Badge variant="outline" className="text-[10px]">
                                {option.category}
                            </Badge>
                        ) : null}
                    </div>
                    {option.description ? (
                        <div className="text-xs text-muted-foreground">
                            {option.description}
                        </div>
                    ) : null}
                    <AcpConfigSelect
                        option={option}
                        disabled={Boolean(acpMutationKey)}
                        onChange={(option, value) => void handleAcpConfigChange(option, value)}
                    />
                </div>
            );
            const statusLabel = (status: string) => {
                if (status === "completed") return "完成";
                if (status === "in_progress") return "进行中";
                return "待处理";
            };

            return (
                <>
                    {combinedHeaderActions}
                    <Popover>
                        <PopoverTrigger asChild>
                            <span>
                                <IconButton
                                    icon={
                                        acpSessionState?.has_active_prompt ? (
                                            <LoaderCircle size={16} className="text-icon animate-spin" />
                                        ) : (
                                            <Bot size={16} className="text-icon" />
                                        )
                                    }
                                    onClick={() => { }}
                                    border
                                    title="ACP 会话控制"
                                    dataAippSlot="chat-conversation-title-acp"
                                />
                            </span>
                        </PopoverTrigger>
                        <PopoverContent align="end" className="w-80 space-y-3">
                            <div className="space-y-1">
                                <div className="flex items-center justify-between gap-2">
                                    <span className="text-sm font-medium">ACP 会话</span>
                                    {acpSessionState?.session_id ? (
                                        <Badge variant="outline">已连接</Badge>
                                    ) : (
                                        <Badge variant="secondary">未启动</Badge>
                                    )}
                                </div>
                                {acpSessionState?.title ? (
                                    <div className="text-xs text-muted-foreground break-all">
                                        {acpSessionState.title}
                                    </div>
                                ) : null}
                                {updatedAtText ? (
                                    <div className="text-xs text-muted-foreground">
                                        最近活动：{updatedAtText}
                                    </div>
                                ) : null}
                                {acpWorkingDirectory ? (
                                    <div className="text-xs text-muted-foreground break-all">
                                        工作目录：{acpWorkingDirectory}
                                    </div>
                                ) : null}
                                {acpSessionState?.session_id &&
                                    !(
                                        acpSessionState.load_session_supported ||
                                        acpSessionState.session_resume_supported
                                    ) ? (
                                    <div className="text-xs text-muted-foreground">
                                        该 Agent 不支持恢复历史 ACP 会话，新会话会使用 AIPP 对话上下文继续。
                                    </div>
                                ) : null}
                            </div>

                            <Separator />

                            <div className="space-y-2">
                                <div className="flex items-center justify-between gap-2">
                                    <span className="text-xs font-medium">会话配置</span>
                                    <Badge variant="secondary" className="text-[10px]">
                                        {configOptionCount} 项
                                    </Badge>
                                </div>
                                {!acpSessionState?.session_id ? (
                                    <div className="text-xs text-muted-foreground">
                                        正在连接 ACP session；连接成功后会读取 Agent 返回的 configOptions。
                                    </div>
                                ) : configOptionCount === 0 ? (
                                    <div className="text-xs text-muted-foreground">
                                        {acpSessionState.restored_session_method
                                            ? `该 Agent 本次通过 session/${acpSessionState.restored_session_method} 恢复，但没有返回可配置的 configOptions。`
                                            : "该 Agent 当前没有返回可配置的 configOptions。"}
                                    </div>
                                ) : (
                                    <div className="space-y-3">
                                        {[modelOption, modeOption, thoughtOption]
                                            .filter((option): option is AcpSessionConfigOption => Boolean(option))
                                            .map(renderAcpConfigSelect)}
                                        {otherOptions.length > 0 ? <Separator /> : null}
                                        {otherOptions.map(renderAcpConfigSelect)}
                                    </div>
                                )}
                            </div>

                            {acpSessionState?.session_id ? (
                                <>
                                    {acpSessionState.plan.length > 0 ? (
                                        <>
                                            <Separator />
                                            <div className="space-y-2">
                                                <div className="text-xs font-medium">执行计划</div>
                                                <div className="space-y-1.5">
                                                    {acpSessionState.plan.map((entry, index) => (
                                                        <div key={`${entry.content}:${index}`} className="flex items-start gap-2 text-xs">
                                                            <Badge variant="outline" className="mt-0.5 text-[10px]">
                                                                {statusLabel(entry.status)}
                                                            </Badge>
                                                            <span className="min-w-0 flex-1 text-muted-foreground">
                                                                {entry.content}
                                                            </span>
                                                        </div>
                                                    ))}
                                                </div>
                                            </div>
                                        </>
                                    ) : null}

                                    {acpSessionState.available_commands.length > 0 ? (
                                        <>
                                            <Separator />
                                            <div className="text-xs text-muted-foreground">
                                                可用 ACP 命令：{acpSessionState.available_commands.length} 个，可在输入框输入 / 选择。
                                            </div>
                                        </>
                                    ) : null}
                                </>
                            ) : null}
                        </PopoverContent>
                    </Popover>
                </>
            );
        }, [
            acpMutationKey,
            acpSessionState,
            acpWorkingDirectory,
            combinedHeaderActions,
            handleAcpConfigChange,
            isAcpAssistant,
        ]);

        const sidebarTodos = useMemo<TodoItem[]>(() => {
            const acpPlanTodos: TodoItem[] = (acpSessionState?.plan ?? []).map((entry) => ({
                content: entry.content,
                status:
                    entry.status === "completed"
                        ? "completed"
                        : entry.status === "in_progress"
                            ? "in_progress"
                            : "pending",
                activeForm: entry.priority ? `ACP ${entry.priority}` : "ACP Plan",
            }));
            return [...acpPlanTodos, ...todos];
        }, [acpSessionState?.plan, todos]);

        // 监听错误通知事件
        useEffect(() => {
            const unsubscribe = listen<{ conversation_id: number | null, error_message: string }>("conversation-window-error-notification", (event) => {
                const { error_message: errorMessage } = event.payload;
                console.error("Received error notification:", errorMessage);

                // 重置AI响应状态
                setAiIsResponsing(false);

                // 使用智能边框控制，而不是直接清空
                updateShiningMessages();
            });

            return () => {
                if (unsubscribe) {
                    unsubscribe.then((f) => f());
                }
            };
        }, [updateShiningMessages]);

        // 在切换对话后，加载完成并渲染出消息后，强制滚动到底部
        useEffect(() => {
            if (virtualizeMessages) return;
            // 必须有对话且不在加载中，且有可显示的消息时才执行
            if (!conversationId) return;
            if (isLoadingShow) return;
            if (allDisplayMessages.length === 0) return;
            // 用户正在通过角标/搜索定位到某条消息时，不要抢回底部
            if (pendingScrollMessageId !== null) return;

            const renderStartTime = performance.now();
            console.log(`[PERF-FRONTEND] 开始渲染 ${allDisplayMessages.length} 条消息`);

            // 等待渲染与布局稳定后再滚动（双 rAF）
            requestAnimationFrame(() =>
                requestAnimationFrame(() => {
                    const renderDuration = performance.now() - renderStartTime;
                    console.log(`[PERF-FRONTEND] 消息渲染完成耗时: ${renderDuration.toFixed(2)}ms`);
                    // 忽略"用户上滑"状态，切换话题后总是瞬时滚动到底部（无平滑动画）
                    smartScroll(true, 'auto');
                })
            );
        }, [conversationId, isLoadingShow, allDisplayMessages.length, pendingScrollMessageId, smartScroll, virtualizeMessages]);

        // 按消息 ID 定位滚动（用于搜索结果）
        useEffect(() => {
            if (virtualizeMessages) {
                return;
            }
            if (pendingScrollMessageId === null) {
                return;
            }
            const container = scrollContainerRef.current;
            if (!container) {
                return;
            }
            const target = container.querySelector(
                `[data-message-id='${pendingScrollMessageId}']`
            ) as HTMLElement | null;
            if (!target) {
                return;
            }
            applyScrollHighlight({
                target,
                messageId: pendingScrollMessageId,
                setShiningMessageIds,
                clearPendingScrollMessageId: setPendingScrollMessageId,
            });
        }, [
            pendingScrollMessageId,
            allDisplayMessages.length,
            scrollContainerRef,
            setShiningMessageIds,
            virtualizeMessages,
        ]);

        useEffect(() => {
            // 用户正在定位到某条消息时，不要抢回底部
            if (pendingScrollMessageId !== null) return;
            const lastMessage = allDisplayMessages[allDisplayMessages.length - 1];
            if (!lastMessage || lastMessage.message_type !== "user") {
                return;
            }
            // Virtuoso 列表在 VirtuosoMessageList 内用 useLayoutEffect + ResizeObserver 钉底
            if (virtualizeMessages && virtualizedListEngine === "virtuoso") {
                return;
            }
            // 在渲染和布局之后执行，避免时间竞态
            requestAnimationFrame(() =>
                requestAnimationFrame(() => {
                    if (virtualizeMessages) {
                        scrollToBottomStable();
                        return;
                    }
                    scrollToUserMessage();
                }),
            );
        }, [
            allDisplayMessages.length,
            pendingScrollMessageId,
            scrollToBottomStable,
            scrollToUserMessage,
            virtualizeMessages,
            virtualizedListEngine,
        ]);

        useEffect(() => {
            if (!inlineInteractionVisible) {
                return;
            }
            requestAnimationFrame(() => {
                smartScroll();
            });
        }, [inlineInteractionVisible, smartScroll]);

        // 移动端虚拟键盘适配：可视高度明显收缩视为键盘弹起，此时用户刚点了输入框，
        // 强制滚动到底部，保证最新消息与输入框可见
        useEffect(() => {
            if (!isMobile || typeof window === "undefined" || !window.visualViewport) {
                return;
            }
            const viewport = window.visualViewport;
            let lastHeight = viewport.height;
            const onResize = () => {
                const shrunk = lastHeight - viewport.height;
                lastHeight = viewport.height;
                if (shrunk > 150) {
                    requestAnimationFrame(() => smartScroll(true, "auto"));
                }
            };
            viewport.addEventListener("resize", onResize);
            return () => viewport.removeEventListener("resize", onResize);
        }, [isMobile, smartScroll]);

        // ============= 组件渲染 =============

        return (
            <ToolErrorContinueProvider value={continueOnToolErrorEnabled}>
                <div
                    ref={dropRef}
                    className={`h-full min-h-0 relative flex bg-background ${isMobile ? '' : 'rounded-xl'}`}
                    data-aipp-slot="chat-conversation-root"
                >
                    {!isMobile && conversationId && (
                        <ConversationTurnRail
                            turns={userTurns}
                            scrollContainerRef={scrollContainerRef}
                            onSelect={(messageId) => {
                                // 先断开存活的 ResizeObserver + 抑制自动滚动，避免刚跳转就被抢回底部
                                handleUserScrollIntent();
                                setPendingScrollMessageId(messageId);
                            }}
                        />
                    )}
                    {/* Main content area：min-h-0 防止嵌套 flex（如总管家）把滚动区撑破导致占位过高 */}
                    <div className="flex min-h-0 min-w-0 flex-1 flex-col" data-aipp-slot="chat-conversation-main">
                        {/* 移动端不显示 ConversationHeader，因为顶部已有菜单栏 */}
                        {!isMobile && !hideHeader && (
                            <ConversationHeader
                                conversationId={conversationId}
                                conversation={conversation}
                                onEdit={openTitleEditDialog}
                                onDelete={handleDeleteConversationSuccess}
                                statsOpen={statsDialogOpen}
                                onStatsOpenChange={setStatsDialogOpen}
                                exportOpen={exportDialogOpen}
                                onExportOpenChange={setExportDialogOpen}
                                allowRename={allowRename}
                                allowDelete={allowDelete}
                                extraActions={acpHeaderActions}
                            />
                        )}

                        <div
                            ref={scrollContainerRef}
                            onWheelCapture={handleUserScrollIntent}
                            onTouchMoveCapture={handleUserScrollIntent}
                            onScroll={virtualizeMessages ? undefined : handleScroll}
                            className={`conversation-scroll-transparent-track min-h-0 h-full flex-1 overflow-y-auto flex flex-col box-border gap-4 ${isMobile ? 'p-3' : 'p-6'}`}
                            data-aipp-slot="chat-conversation-scroll"
                        >
                            <ConversationContent
                                conversationId={conversationId}
                                // MessageList props
                                allDisplayMessages={allDisplayMessages}
                                streamingMessages={streamingMessages}
                                shiningMessageIds={shiningMessageIds}
                                shiningMcpCallId={shiningMcpCallId}
                                reasoningExpandStates={reasoningExpandStates}
                                mcpToolCallStates={mcpToolCallStates}
                                agentActivities={displayAgentActivities}
                                generationGroups={messageGroupsData.generationGroups}
                                selectedVersions={messageGroupsData.selectedVersions}
                                getGenerationGroupControl={messageGroupsData.getGenerationGroupControl}
                                handleGenerationVersionChange={messageGroupsData.handleGenerationVersionChange}
                                onCodeRun={handleArtifact}
                                onMessageRegenerate={handleMessageRegenerate}
                                onMessageEdit={handleMessageEdit}
                                onMessageFork={handleMessageFork}
                                onQueuedMessagePromote={handleQueuedMessagePromote}
                                onToggleReasoningExpand={toggleReasoningExpand}
                                inlineInteractionItems={conversationId ? inlineInteractionItems : undefined}
                                allowFeishuDebugResend={allowFeishuDebugResend}
                                renderMessageActions={renderPluginMessageActions}
                                virtualizeMessages={virtualizeMessages}
                                virtualizedListEngine={virtualizedListEngine}
                                scrollContainerRef={scrollContainerRef}
                                pendingScrollMessageId={pendingScrollMessageId}
                                clearPendingScrollMessageId={setPendingScrollMessageId}
                                setShiningMessageIds={setShiningMessageIds}
                                onScrollStateChange={syncScrollState}
                                smartScroll={smartScroll}
                                // NewChatComponent props
                                selectedText={selectedText}
                                selectedAssistant={selectedAssistant}
                                assistants={assistants}
                                setSelectedAssistant={setSelectedAssistant}
                            />
                            <div ref={messagesEndRef} data-aipp-slot="chat-messages-end-anchor" />
                        </div>

                        {isDragging ? <FileDropArea onDragChange={setIsDragging} onFilesSelect={handleDropFiles} /> : null}

                        <InputArea
                            ref={inputAreaRef}
                            inputText={inputText}
                            setInputText={setInputText}
                            fileInfoList={fileInfoList}
                            handleChooseFile={handleChooseFile}
                            handleDeleteFile={handleDeleteFile}
                            handlePaste={handlePaste}
                            handleSend={handleSend}
                            aiIsResponsing={effectiveAiIsResponsing}
                            placement="bottom"
                            isMobile={isMobile}
                            sidebarWidth={sidebarWidth}
                            sidebarVisible={!isMobile && !hideSidebar && Boolean(conversationId)}
                            sendButtonIcon={sendButtonIconSlot}
                            sendButtonVisual={sendButtonVisualSlot}
                            acpAvailableCommands={acpSessionState?.available_commands ?? []}
                        />
                    </div>

                    {/* Right sidebar - only show on desktop when sidebar window is not open */}
                    {!isMobile && !hideSidebar && conversationId && !sidebarWindowOpen && (
                        <ChatSidebar
                            todos={sidebarTodos}
                            artifacts={artifacts}
                            contextItems={contextItems}
                            conversationId={conversationId}
                            pluginList={pluginList}
                            toggleRequestVersion={sidebarToggleRequestVersion}
                            focusedContextId={focusedContextId}
                            onExpandChange={handleSidebarExpandChange}
                            onOpenWindow={handleOpenSidebarWindow}
                            onArtifactClick={(artifact) => handleArtifact(artifact.language, artifact.code)}
                            onPreviewFileClick={handlePreviewFileContextClick}
                        />
                    )}

                    <ConversationTitleEditDialog
                        isOpen={titleEditDialogIsOpen}
                        conversationId={conversation?.id || 0}
                        initialTitle={conversation?.name || ""}
                        onClose={closeTitleEditDialog}
                    />

                    <MessageEditDialog
                        isOpen={editDialogIsOpen}
                        initialContent={editingMessage?.content || ""}
                        messageType={editingMessage?.message_type || ""}
                        onClose={closeEditDialog}
                        onSave={handleEditSave}
                        onSaveAndRegenerate={handleEditSaveAndRegenerate}
                    />

                    {isLoadingShow ? (
                        <div
                            className="bg-background/95 w-full h-full absolute flex items-center justify-center backdrop-blur rounded-xl"
                            data-aipp-slot="chat-loading-overlay"
                        >
                            <div className="loading-icon"></div>
                            <div className="text-primary text-base font-medium">加载中...</div>
                        </div>
                    ) : null}
                </div>
            </ToolErrorContinueProvider>
        );
    }
);

export default ConversationUI;
