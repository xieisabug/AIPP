import React, { useMemo } from "react";
import MessageItem from "../MessageItem";
import VersionPagination from "../VersionPagination";
import { Message, StreamEvent } from "../../data/Conversation";
import type { InlineInteractionItem } from "../ConversationUI";

export interface MessageListProps {
    allDisplayMessages: Message[];
    streamingMessages: Map<number, StreamEvent>;
    shiningMessageIds: Set<number>;
    reasoningExpandStates: Map<number, boolean>;
    mcpToolCallStates: Map<number, any>;
    generationGroups: Map<string, any>;
    selectedVersions: Map<string, number>;
    getGenerationGroupControl: (message: Message) => any;
    handleGenerationVersionChange: (groupId: string, versionIndex: number) => void;
    onCodeRun: (lang: string, inputStr: string) => void;
    onMessageRegenerate: (messageId: number) => void;
    onMessageEdit: (message: Message) => void;
    onMessageFork: (messageId: number) => void;
    onToggleReasoningExpand: (messageId: number) => void;
    inlineInteractionItems?: InlineInteractionItem[];
}

const MessageList: React.FC<MessageListProps> = ({
    allDisplayMessages,
    streamingMessages,
    shiningMessageIds,
    reasoningExpandStates,
    mcpToolCallStates,
    generationGroups,
    selectedVersions,
    getGenerationGroupControl,
    handleGenerationVersionChange,
    onCodeRun,
    onMessageRegenerate,
    onMessageEdit,
    onMessageFork,
    onToggleReasoningExpand,
    inlineInteractionItems,
}) => {
    const messageInlineInteractionMap = useMemo(() => {
        const map = new Map<number, InlineInteractionItem[]>();
        (inlineInteractionItems ?? []).forEach((item) => {
            if (item.messageId === undefined || item.messageId === null) {
                return;
            }
            const existing = map.get(item.messageId) ?? [];
            map.set(item.messageId, [...existing, item]);
        });
        return map;
    }, [inlineInteractionItems]);

    const displayedMessageIdSet = useMemo(
        () => new Set(allDisplayMessages.map((message) => message.id)),
        [allDisplayMessages]
    );

    const fallbackInlineInteractionItems = useMemo(() => {
        return (inlineInteractionItems ?? []).filter(
            (item) =>
                item.messageId === undefined ||
                item.messageId === null ||
                !displayedMessageIdSet.has(item.messageId)
        );
    }, [inlineInteractionItems, displayedMessageIdSet]);

    // 将消息渲染逻辑拆分为更小的部分
    const messageElements = useMemo(() => {
        const t0 = performance.now();
        // 计算最后一条消息的 ID
        const lastMessageId = allDisplayMessages.length > 0
            ? allDisplayMessages[allDisplayMessages.length - 1].id
            : -1;

        const elements = allDisplayMessages.map((message) => {
            // 查找对应的流式消息信息（如果存在）
            const streamEvent = streamingMessages.get(message.id);

            // 检查是否需要显示版本控制
            const groupControl = getGenerationGroupControl(message);

            // 检查是否需要显示shine-border
            const shouldShowShineBorder = shiningMessageIds.has(message.id);

            // 判断是否为最后一条消息
            const isLastMessage = message.id === lastMessageId;

            return {
                messageId: message.id,
                messageElement: (
                    <MessageItem
                        key={`message-${message.id}`}
                        message={message}
                        streamEvent={streamEvent}
                        onCodeRun={onCodeRun}
                        onMessageRegenerate={() => onMessageRegenerate(message.id)}
                        onMessageEdit={() => onMessageEdit(message)}
                        onMessageFork={() => onMessageFork(message.id)}
                        // Reasoning 展开状态相关 props
                        isReasoningExpanded={
                            reasoningExpandStates.get(message.id) || false
                        }
                        onToggleReasoningExpand={() =>
                            onToggleReasoningExpand(message.id)
                        }
                        // ShineBorder 动画状态
                        shouldShowShineBorder={shouldShowShineBorder}
                        // MCP 工具调用需要的上下文信息
                        conversationId={message.conversation_id}
                        // 传递 MCP 工具调用状态
                        mcpToolCallStates={mcpToolCallStates}
                        // 防泄露模式：是否为最后一条消息
                        isLastMessage={isLastMessage}
                        inlineInteractionItems={messageInlineInteractionMap.get(message.id)}
                    />
                ),
                groupControl,
            };
        });
        const dt = performance.now() - t0;
        console.log(`[PERF-FRONTEND] MessageList 构建元素耗时: ${dt.toFixed(2)}ms (count: ${elements.length})`);
        return elements;
    }, [
        allDisplayMessages,
        streamingMessages,
        shiningMessageIds,
        reasoningExpandStates,
        mcpToolCallStates,
        getGenerationGroupControl,
        onCodeRun,
        onMessageRegenerate,
        onMessageEdit,
        onToggleReasoningExpand,
        onMessageFork,
        messageInlineInteractionMap,
    ]);

    // 优化版本控制组件的渲染
    const versionControlElements = useMemo(() => {
        return messageElements
            .filter(({ groupControl }) => groupControl)
            .map(({ messageId, groupControl }) => (
                <div key={`version-${messageId}`} className="flex justify-start mt-2">
                    <VersionPagination
                        currentVersion={groupControl.currentVersion}
                        totalVersions={groupControl.totalVersions}
                        onVersionChange={(versionIndex) =>
                            handleGenerationVersionChange(
                                groupControl.groupId,
                                versionIndex,
                            )
                        }
                    />
                </div>
            ));
    }, [messageElements, handleGenerationVersionChange]);

    // 建立版本控制元素的快速索引映射，避免重复查找
    const versionMap = useMemo(() => {
        const map = new Map<string, React.ReactElement>();
        versionControlElements.forEach((el) => {
            const key = el.key != null ? String(el.key) : "";
            if (key) map.set(key, el);
        });
        return map;
    }, [versionControlElements]);

    // 优化占位符消息的渲染
    const placeholderElements = useMemo(() => {
        const placeholders: React.ReactElement[] = [];
        
        generationGroups.forEach((group, groupId) => {
            const selectedVersionIndex =
                selectedVersions.get(groupId) ??
                (group.versions.length > 0 ? group.versions.length - 1 : 0);
            const selectedVersionData = group.versions[selectedVersionIndex];

            // 如果选中的是占位符版本，添加占位符消息
            if (selectedVersionData?.isPlaceholder) {
                placeholders.push(
                    <React.Fragment key={`placeholder_${groupId}`}>
                        <div className="flex justify-start mb-4">
                            <div className="bg-muted rounded-lg p-4 max-w-3xl">
                                <div className="flex items-center space-x-2">
                                    <div className="animate-spin rounded-full h-4 w-4 border-b-2 border-foreground"></div>
                                    <span className="text-sm text-muted-foreground">
                                        正在重新生成...
                                    </span>
                                </div>
                            </div>
                        </div>
                        <div className="flex justify-start mt-2">
                            <VersionPagination
                                currentVersion={selectedVersionIndex + 1}
                                totalVersions={group.versions.length}
                                onVersionChange={(versionIndex) =>
                                    handleGenerationVersionChange(
                                        groupId,
                                        versionIndex,
                                    )
                                }
                            />
                        </div>
                    </React.Fragment>
                );
            }
        });

        return placeholders;
    }, [generationGroups, selectedVersions, handleGenerationVersionChange]);

    // 组合所有元素，并将最后的 user + AI 响应包裹在带 min-height 的容器中
    const allElements = useMemo(() => {
        const elements: React.ReactElement[] = [];

        // 查找最后一条 user 消息的索引
        let lastUserMessageIndex = -1;
        for (let i = allDisplayMessages.length - 1; i >= 0; i--) {
            if (allDisplayMessages[i].message_type === 'user') {
                lastUserMessageIndex = i;
                break;
            }
        }

        if (lastUserMessageIndex >= 0) {
            const before = messageElements.slice(0, lastUserMessageIndex);
            const last = messageElements.slice(lastUserMessageIndex);

            // 渲染最后一组之前的消息及其版本控制
            before.forEach((item, i) => {
                elements.push(item.messageElement);
                const ve = versionMap.get(`version-${messageElements[i].messageId}`);
                if (ve) elements.push(ve);
            });

            // 渲染最后一组，放入容器中
            elements.push(
                <div
                    key="last-reply-container"
                    id="last-reply-container"
                    style={{ minHeight: 'calc(100dvh - 130px)' }}
                    className="flex flex-col gap-4"
                >
                    {last.map((item, idx) => (
                        <React.Fragment key={`last-group-${messageElements[lastUserMessageIndex + idx].messageId}`}>
                            {item.messageElement}
                            {versionMap.get(`version-${messageElements[lastUserMessageIndex + idx].messageId}`) || null}
                        </React.Fragment>
                    ))}
                    {placeholderElements}
                    {fallbackInlineInteractionItems.length > 0 && (
                        <div className="flex flex-col gap-4 pt-2">
                            {fallbackInlineInteractionItems.map((item) => (
                                <React.Fragment key={item.key}>{item.content}</React.Fragment>
                            ))}
                        </div>
                    )}
                    <div className="flex-none h-[120px]"></div>
                </div>
            );
        } else {
            // 如果没有找到 user 消息（比如空对话），添加占位符
            if (placeholderElements.length > 0) {
                elements.push(...placeholderElements);
            }
        }

        return elements;
    }, [
        messageElements,
        versionMap,
        placeholderElements,
        allDisplayMessages,
        fallbackInlineInteractionItems,
    ]);

    return <>{allElements}</>;
};

export default React.memo(MessageList);
