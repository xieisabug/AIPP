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
    StreamEvent,
    ConversationWithMessages,
    GroupMergeEvent,
    MCPToolCallUpdateEvent,
    AcpSessionConfigOption,
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
import ConversationContent from "./conversation/ConversationContent";
import { applyScrollHighlight } from "./conversation/scrollHighlight";
import IconButton from "./IconButton";
import { Badge } from "@/components/ui/badge";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import {
    Select,
    SelectContent,
    SelectItem,
    SelectTrigger,
    SelectValue,
} from "@/components/ui/select";
import { Separator } from "@/components/ui/separator";
import { Bot, LoaderCircle } from "lucide-react";
import { toast } from "sonner";

// 导入 Chat Sidebar 相关
import { ChatSidebar } from "./chat-sidebar";
import { useTodoList } from "@/hooks/useTodoList";
import { useArtifactExtractor } from "@/hooks/useArtifactExtractor";
import { useExplicitArtifacts } from "@/hooks/useExplicitArtifacts";
import { useContextList } from "@/hooks/useContextList";
import { mergeMessagesWithStreamingState } from "@/utils/streamingMessageState";

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
    windowLabel?: string;
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
            windowLabel = "chat_ui",
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

        // 常规消息列表
        const [messages, setMessages] = useState<Array<Message>>([]);
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
            shiningMcpCallId,
            runtimeState,
            updateShiningMessages,
            updateFunctionMap,
            clearStreamingMessages,
            clearShiningMessages,
            setPendingUserMessage,
            acpSessionState,
        } = useConversationEvents(conversationEventsOptions);

        const [acpMutationKey, setAcpMutationKey] = useState<string | null>(null);

        useEffect(() => {
            streamingMessagesRef.current = streamingMessages;
        }, [streamingMessages]);

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

        // 当 functionMap 变化时更新事件处理器
        useEffect(() => {
            updateFunctionMap(functionMap);
        }, [functionMap, updateFunctionMap]);

        // 消息处理 - 首先需要获取 groupMergeMap
        const [groupMergeMap, setGroupMergeMap] = useState<Map<string, string>>(new Map());

        // 第一步：消息处理 - 获取合并的消息用于分组
        const { combinedMessagesForGrouping } = useMessageProcessing({
            messages,
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
            messages,
            streamingMessages,
            conversation,
            generationGroups: messageGroupsData.generationGroups,
            groupRootMessageIds: messageGroupsData.groupRootMessageIds,
            getMessageVersionInfo: messageGroupsData.getMessageVersionInfo,
        });
        // 滚动管理 - 移除依赖项，改为手动调用
        const {
            messagesEndRef,
            scrollContainerRef,
            handleScroll,
            handleUserScrollIntent,
            syncScrollState,
            smartScroll,
            scrollToUserMessage,
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

        // ============= Sidebar Window 事件处理 =============
        
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

        // Fetch ACP working directory for current assistant
        useEffect(() => {
            const assistantId = conversation?.assistant_id ?? selectedAssistant;
            if (!assistantId) {
                setAcpWorkingDirectory(null);
                return;
            }

            invoke<string>("get_acp_working_directory", { assistantId })
                .then((workingDirectory) => {
                    setAcpWorkingDirectory(workingDirectory);
                })
                .catch(() => {
                    setAcpWorkingDirectory(null);
                });
        }, [conversation?.assistant_id, selectedAssistant, assistants]);

        const handleAcpModeChange = useCallback(
            async (modeId: string) => {
                const conversationIdNum = Number(conversationId);
                if (!conversationIdNum || Number.isNaN(conversationIdNum)) {
                    return;
                }
                setAcpMutationKey(`mode:${modeId}`);
                try {
                    await invoke("set_acp_session_mode", {
                        conversationId: conversationIdNum,
                        modeId,
                    });
                } catch (error) {
                    toast.error(`切换 ACP 模式失败: ${String(error)}`);
                } finally {
                    setAcpMutationKey(null);
                }
            },
            [conversationId]
        );

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
                    toast.error(`更新 ACP 配置失败: ${String(error)}`);
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
            }),
            [
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
            const isAcpConversation = Boolean(acpWorkingDirectory) || Boolean(acpSessionState);
            if (!isAcpConversation) {
                return combinedHeaderActions;
            }

            const currentMode = acpSessionState?.modes.find(
                (mode) => mode.id === acpSessionState.current_mode_id
            );
            const hasModeConfigOption = acpSessionState?.config_options.some(
                (option) => option.category === "mode"
            );
            const updatedAtText = acpSessionState?.updated_at
                ? new Date(acpSessionState.updated_at).toLocaleString()
                : null;

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
                            </div>

                            {!acpSessionState?.session_id ? (
                                <div className="text-xs text-muted-foreground">
                                    首次发送消息后才会创建 ACP 会话。
                                </div>
                            ) : (
                                <>
                                    {currentMode ? (
                                        <div className="flex items-center gap-2 text-xs text-muted-foreground">
                                            <span>当前模式</span>
                                            <Badge variant="outline">{currentMode.name}</Badge>
                                        </div>
                                    ) : null}

                                    {!hasModeConfigOption && acpSessionState.modes.length > 0 ? (
                                        <div className="space-y-1.5">
                                            <div className="text-xs font-medium">切换模式</div>
                                            <Select
                                                value={acpSessionState.current_mode_id ?? undefined}
                                                onValueChange={(value) => void handleAcpModeChange(value)}
                                                disabled={Boolean(acpMutationKey)}
                                            >
                                                <SelectTrigger className="w-full">
                                                    <SelectValue placeholder="选择模式" />
                                                </SelectTrigger>
                                                <SelectContent>
                                                    {acpSessionState.modes.map((mode) => (
                                                        <SelectItem key={mode.id} value={mode.id}>
                                                            {mode.name}
                                                        </SelectItem>
                                                    ))}
                                                </SelectContent>
                                            </Select>
                                        </div>
                                    ) : null}

                                    {acpSessionState.config_options.length > 0 ? <Separator /> : null}

                                    <div className="space-y-3">
                                        {acpSessionState.config_options.map((option) => (
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
                                                <Select
                                                    value={option.current_value}
                                                    onValueChange={(value) => void handleAcpConfigChange(option, value)}
                                                    disabled={Boolean(acpMutationKey)}
                                                >
                                                    <SelectTrigger className="w-full">
                                                        <SelectValue placeholder="选择配置" />
                                                    </SelectTrigger>
                                                    <SelectContent>
                                                        {option.options.map((choice) => (
                                                            <SelectItem key={`${option.id}:${choice.value}`} value={choice.value}>
                                                                {choice.group_name
                                                                    ? `${choice.group_name} / ${choice.name}`
                                                                    : choice.name}
                                                            </SelectItem>
                                                        ))}
                                                    </SelectContent>
                                                </Select>
                                            </div>
                                        ))}
                                    </div>
                                </>
                            )}
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
            handleAcpModeChange,
        ]);

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
            // 必须有对话且不在加载中，且有可显示的消息时才执行
            if (!conversationId) return;
            if (isLoadingShow) return;
            if (allDisplayMessages.length === 0) return;

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
        }, [conversationId, isLoadingShow, allDisplayMessages.length, smartScroll]);

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
            const lastMessage = allDisplayMessages[allDisplayMessages.length - 1];
            if (lastMessage && lastMessage.message_type === 'user') {
                // 在渲染和布局之后执行，避免时间竞态
                requestAnimationFrame(() =>
                    requestAnimationFrame(() => {
                        if (virtualizeMessages) {
                            smartScroll(true, "smooth");
                            return;
                        }
                        scrollToUserMessage();
                    })
                );
            }
        }, [
            allDisplayMessages.length,
            scrollToUserMessage,
            smartScroll,
            virtualizeMessages,
        ]);

        useEffect(() => {
            if (!inlineInteractionVisible) {
                return;
            }
            requestAnimationFrame(() => {
                smartScroll();
            });
        }, [inlineInteractionVisible, smartScroll]);

        // ============= 组件渲染 =============

        return (
            <div
                ref={dropRef}
                className={`h-full relative flex bg-background ${isMobile ? '' : 'rounded-xl'}`}
                data-aipp-slot="chat-conversation-root"
            >
                {/* Main content area */}
                <div className="flex-1 flex flex-col min-w-0" data-aipp-slot="chat-conversation-main">
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
                        className={`conversation-scroll-transparent-track h-full flex-1 overflow-y-auto flex flex-col box-border gap-4 ${isMobile ? 'p-3' : 'p-6'}`}
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
                            generationGroups={messageGroupsData.generationGroups}
                            selectedVersions={messageGroupsData.selectedVersions}
                            getGenerationGroupControl={messageGroupsData.getGenerationGroupControl}
                            handleGenerationVersionChange={messageGroupsData.handleGenerationVersionChange}
                            onCodeRun={handleArtifact}
                            onMessageRegenerate={handleMessageRegenerate}
                            onMessageEdit={handleMessageEdit}
                            onMessageFork={handleMessageFork}
                            onToggleReasoningExpand={toggleReasoningExpand}
                            inlineInteractionItems={conversationId ? inlineInteractionItems : undefined}
                            allowFeishuDebugResend={allowFeishuDebugResend}
                            virtualizeMessages={virtualizeMessages}
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
                    />
                </div>

                {/* Right sidebar - only show on desktop when sidebar window is not open */}
                {!isMobile && !hideSidebar && conversationId && !sidebarWindowOpen && (
                        <ChatSidebar
                            todos={todos}
                            artifacts={artifacts}
                            contextItems={contextItems}
                            conversationId={conversationId}
                            pluginList={pluginList}
                            toggleRequestVersion={sidebarToggleRequestVersion}
                            onExpandChange={handleSidebarExpandChange}
                            onOpenWindow={handleOpenSidebarWindow}
                            onArtifactClick={(artifact) => handleArtifact(artifact.language, artifact.code)}
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
        );
    }
);

export default ConversationUI;
