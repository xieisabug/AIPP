import React, { useMemo } from "react";

import {
    useMessageListElements,
    type UseMessageListElementsProps,
} from "./useMessageListElements";

export interface MessageListProps extends UseMessageListElementsProps {}

const MessageList: React.FC<MessageListProps> = ({
    allDisplayMessages,
    ...messageListProps
}) => {
    const {
        fallbackInlineInteractionItems,
        messageElements,
        placeholderElements,
        versionMap,
    } = useMessageListElements({
        allDisplayMessages,
        ...messageListProps,
    });

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
                    data-aipp-slot="chat-last-reply-container"
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
