import { invoke } from "@tauri-apps/api/core";
import {
    useCallback,
    useEffect,
    useMemo,
    useRef,
    useState,
    forwardRef,
    useImperativeHandle,
    useLayoutEffect,
} from "react";

import {
    Conversation,
    Message,
    StreamEvent,
    ConversationWithMessages,
    GroupMergeEvent,
    MCPToolCallUpdateEvent,
} from "../data/Conversation";
import "katex/dist/katex.min.css";
import { listen } from "@tauri-apps/api/event";
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

// 导入 Chat Sidebar 相关
import { ChatSidebar } from "./chat-sidebar";
import { useTodoList } from "@/hooks/useTodoList";
import { useArtifactExtractor } from "@/hooks/useArtifactExtractor";
import { useContextList } from "@/hooks/useContextList";

// 暴露给外部的方法接口
export interface ConversationUIRef {
    focus: () => void;
    scrollToMessage: (messageId: number) => void;
}

interface ConversationUIProps {
    conversationId: string;
    onChangeConversationId: (conversationId: string) => void;
    pluginList: any[];
    isMobile?: boolean;
    onConversationChange?: (conversation?: Conversation) => void;
}

const ConversationUI = forwardRef<ConversationUIRef, ConversationUIProps>(
    ({ conversationId, onChangeConversationId, pluginList, isMobile = false, onConversationChange }, ref) => {
        // ============= 基础状态管理 =============

        // 当前对话信息和助手列表
        const [conversation, setConversation] = useState<Conversation>();
        const [assistants, setAssistants] = useState<AssistantListItem[]>([]);
        const [selectedAssistant, setSelectedAssistant] = useState(-1);

        // 对话加载状态
        const [isLoadingShow, setIsLoadingShow] = useState(false);

        // 常规消息列表
        const [messages, setMessages] = useState<Array<Message>>([]);

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

        // Sidebar expansion state
        const [sidebarExpanded, setSidebarExpanded] = useState(false);

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
                        setMessages(updatedConversation.messages);
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
            [conversationId, setFunctionMapForMessage, resetReveal]
        );

        const handleGroupMerge = useCallback((groupMergeData: GroupMergeEvent) => {
            // 设置组合并关系
            setGroupMergeMap((prev) => {
                const newMap = new Map(prev);
                newMap.set(groupMergeData.new_group_id, groupMergeData.original_group_id);
                return newMap;
            });
        }, []);

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
                // 检查messages中是否已存在该消息
                setMessages((prevMessages) => {
                    const existingIndex = prevMessages.findIndex((msg) => msg.id === streamEvent.message_id);

                    if (existingIndex !== -1) {
                        // 消息已存在，更新其内容和完成状态
                        const updatedMessages = [...prevMessages];
                        const existingMessage = updatedMessages[existingIndex];

                        // 如果事件中包含 Token 计数，则更新
                        const tokenUpdates = (streamEvent.token_count !== undefined ||
                                            streamEvent.input_token_count !== undefined ||
                                            streamEvent.output_token_count !== undefined)
                            ? {
                                token_count: streamEvent.token_count ?? existingMessage.token_count,
                                input_token_count: streamEvent.input_token_count ?? existingMessage.input_token_count,
                                output_token_count: streamEvent.output_token_count ?? existingMessage.output_token_count,
                              }
                            : {};

                        // 如果事件中包含性能指标，则更新
                        const performanceUpdates = (streamEvent.ttft_ms !== undefined ||
                                                    streamEvent.tps !== undefined)
                            ? {
                                ttft_ms: streamEvent.ttft_ms ?? existingMessage.ttft_ms,
                                tps: streamEvent.tps ?? existingMessage.tps,
                              }
                            : {};

                        updatedMessages[existingIndex] = {
                            ...existingMessage,
                            content: streamEvent.content,
                            message_type: streamEvent.message_type,
                            finish_time: new Date(), // 标记为完成
                            ...tokenUpdates, // 如果有 Token 计数，则更新
                            ...performanceUpdates, // 如果有性能指标，则更新
                        };
                        return updatedMessages;
                    } else {
                        // 消息不存在，添加新消息
                        const lastMessage = prevMessages[prevMessages.length - 1];
                        const baseTime = lastMessage ? new Date(lastMessage.created_time) : new Date();
                        const newMessage: Message = {
                            id: streamEvent.message_id,
                            conversation_id: conversation?.id || 0,
                            message_type: streamEvent.message_type,
                            content: streamEvent.content,
                            llm_model_id: null,
                            created_time: new Date(baseTime.getTime() + 1000),
                            start_time: streamEvent.message_type === "reasoning" ? baseTime : null,
                            finish_time: new Date(), // 标记为完成
                            // 如果事件中包含 Token 计数，则使用，否则默认为 0
                            token_count: streamEvent.token_count ?? 0,
                            input_token_count: streamEvent.input_token_count ?? 0,
                            output_token_count: streamEvent.output_token_count ?? 0,
                            generation_group_id: null, // 流式消息暂时不设置generation_group_id
                            parent_group_id: null, // 流式消息暂时不设置parent_group_id
                            regenerate: null,
                        };
                        return [...prevMessages, newMessage];
                    }
                });
            },
            [conversation?.id]
        );

        // 滚动管理 - 移除依赖项，改为手动调用
        const { messagesEndRef, scrollContainerRef, handleScroll, smartScroll, scrollToUserMessage } = useScrollManagement();
        const [pendingScrollMessageId, setPendingScrollMessageId] = useState<number | null>(null);

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
                setTimeout(() => smartScroll(), 0);
            };

            return {
                conversationId: conversationId,
                onMessageAdd: handleMessageAdd,
                onMessageUpdate: handleMessageUpdate,
                onGroupMerge: handleGroupMerge,
                onMCPToolCallUpdate: handleMCPToolCallUpdate,
                onAiResponseComplete: handleAiResponseComplete,
                onError: handleError,
            };
        }, [
            conversationId,
            handleMessageAdd,
            handleGroupMerge,
            handleMCPToolCallUpdate,
            handleAiResponseComplete,
            handleError,
            handleMessageCompletion,
            smartScroll,
            // 移除 functionMap 依赖，改为在回调内部访问
        ]);

        // 使用共享的消息事件处理 hook
        const {
            streamingMessages,
            shiningMessageIds,
            setShiningMessageIds,
            setManualShineMessage,
            mcpToolCallStates,
            updateShiningMessages,
            updateFunctionMap,
            clearStreamingMessages,
            clearShiningMessages,
            setPendingUserMessage,
        } = useConversationEvents(conversationEventsOptions);

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

        // ============= Chat Sidebar 数据提取 =============
        
        // Artifacts from messages (code blocks)
        const { artifacts } = useArtifactExtractor({
            messages: allDisplayMessages,
        });

        // Context items (user files + MCP tool calls + message attachments)
        const { contextItems } = useContextList({
            userFiles: fileInfoList,
            mcpToolCallStates,
            messages,
        });

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
            aiIsResponsing,
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
            }),
            []
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
            requestAnimationFrame(() => {
                target.scrollIntoView({ behavior: "smooth", block: "center" });
                setShiningMessageIds(() => new Set([pendingScrollMessageId]));
                setTimeout(() => {
                    setShiningMessageIds(new Set());
                }, 2000);
                setPendingScrollMessageId(null);
            });
        }, [pendingScrollMessageId, allDisplayMessages.length, scrollContainerRef, setShiningMessageIds]);

        useEffect(() => {
            const lastMessage = allDisplayMessages[allDisplayMessages.length - 1];
            if (lastMessage && lastMessage.message_type === 'user') {
                // 在渲染和布局之后执行，避免时间竞态
                requestAnimationFrame(() =>
                    requestAnimationFrame(() => {
                        scrollToUserMessage();
                    })
                );
            }
        }, [allDisplayMessages.length, scrollToUserMessage]);

        // ============= 组件渲染 =============

        return (
            <div ref={dropRef} className={`h-full relative flex bg-background ${isMobile ? '' : 'rounded-xl'}`}>
                {/* Main content area */}
                <div className="flex-1 flex flex-col min-w-0">
                    {/* 移动端不显示 ConversationHeader，因为顶部已有菜单栏 */}
                    {!isMobile && (
                        <ConversationHeader
                            conversationId={conversationId}
                            conversation={conversation}
                            onEdit={openTitleEditDialog}
                            onDelete={handleDeleteConversationSuccess}
                        />
                    )}

                    <div
                        ref={scrollContainerRef}
                        onScroll={handleScroll}
                        className={`h-full flex-1 overflow-y-auto flex flex-col box-border gap-4 ${isMobile ? 'p-3' : 'p-6'}`}
                    >
                        <ConversationContent
                            conversationId={conversationId}
                            // MessageList props
                            allDisplayMessages={allDisplayMessages}
                            streamingMessages={streamingMessages}
                            shiningMessageIds={shiningMessageIds}
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
                            // NewChatComponent props
                            selectedText={selectedText}
                            selectedAssistant={selectedAssistant}
                            assistants={assistants}
                            setSelectedAssistant={setSelectedAssistant}
                        />
                        <div ref={messagesEndRef} />
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
                            aiIsResponsing={aiIsResponsing}
                            placement="bottom"
                            isMobile={isMobile}
                            sidebarExpanded={sidebarExpanded}
                            sidebarVisible={!isMobile && Boolean(conversationId)}
                        />
                </div>

                {/* Right sidebar - only show on desktop */}
                {!isMobile && conversationId && (
                    <ChatSidebar
                        todos={todos}
                        artifacts={artifacts}
                        contextItems={contextItems}
                        conversationId={conversationId}
                        onExpandChange={setSidebarExpanded}
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
                    <div className="bg-background/95 w-full h-full absolute flex items-center justify-center backdrop-blur rounded-xl">
                        <div className="loading-icon"></div>
                        <div className="text-primary text-base font-medium">加载中...</div>
                    </div>
                ) : null}
            </div>
        );
    }
);

export default ConversationUI;
