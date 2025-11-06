import React, { useMemo, Profiler } from "react";
import MessageItem from "../MessageItem";
import VersionPagination from "../VersionPagination";
import { Message, StreamEvent } from "../../data/Conversation";
import { onRenderCallback } from "../../hooks/usePerformanceMonitor";

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
}

// 使用 React.memo 优化 MessageItem 渲染
const MemoizedMessageItem = React.memo(MessageItem);

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
}) => {
    // 将消息渲染逻辑拆分为更小的部分
    const messageElements = useMemo(() => {
        const t0 = performance.now();
        const elements = allDisplayMessages.map((message) => {
            // 查找对应的流式消息信息（如果存在）
            const streamEvent = streamingMessages.get(message.id);

            // 检查是否需要显示版本控制
            const groupControl = getGenerationGroupControl(message);

            // 检查是否需要显示shine-border
            const shouldShowShineBorder = shiningMessageIds.has(message.id);

            return {
                messageId: message.id,
                messageElement: (
                    <Profiler id={`MessageItem-${message.id}`} onRender={onRenderCallback as any} key={`message-${message.id}`}>
                        <MemoizedMessageItem
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
                        />
                    </Profiler>
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

            // 渲染最后一组，放入容器中，保证最小高度
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
    }, [messageElements, versionMap, placeholderElements, allDisplayMessages]);

    return (
        <Profiler id="MessageList" onRender={onRenderCallback as any}>
            {allElements}
        </Profiler>
    );
};

export default React.memo(MessageList);
