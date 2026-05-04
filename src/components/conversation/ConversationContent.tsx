import React, { memo } from "react";
import MessageList from "./MessageList";
import VirtualizedMessageList from "./VirtualizedMessageList";
import NewChatComponent from "../NewChatComponent";
import { Message, StreamEvent } from "../../data/Conversation";
import { AssistantListItem } from "../../data/Assistant";
import type { InlineInteractionItem } from "../ConversationUI";

export interface ConversationContentProps {
    conversationId: string;
    // MessageList props
    allDisplayMessages: Message[];
    streamingMessages: Map<number, StreamEvent>;
    shiningMessageIds: Set<number>;
    shiningMcpCallId: number | null;
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
    onQueuedMessagePromote?: (queueId: number) => void;
    onToggleReasoningExpand: (messageId: number) => void;
    inlineInteractionItems?: InlineInteractionItem[];
    allowFeishuDebugResend?: boolean;
    virtualizeMessages?: boolean;
    scrollContainerRef?: React.RefObject<HTMLDivElement | null>;
    pendingScrollMessageId?: number | null;
    clearPendingScrollMessageId?: (messageId: number | null) => void;
    setShiningMessageIds?: React.Dispatch<React.SetStateAction<Set<number>>>;
    onScrollStateChange?: (container?: HTMLDivElement | null) => void;
    smartScroll?: (forceScroll?: boolean, behaviorOverride?: ScrollBehavior) => void;
    // NewChatComponent props
    selectedText: string;
    selectedAssistant: number;
    assistants: AssistantListItem[];
    setSelectedAssistant: (assistantId: number) => void;
}

const ConversationContent: React.FC<ConversationContentProps> = memo(({
    conversationId,
    // MessageList props
    allDisplayMessages,
    streamingMessages,
    shiningMessageIds,
    shiningMcpCallId,
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
    onQueuedMessagePromote,
    onToggleReasoningExpand,
    inlineInteractionItems,
    allowFeishuDebugResend = false,
    virtualizeMessages = false,
    scrollContainerRef,
    pendingScrollMessageId = null,
    clearPendingScrollMessageId,
    setShiningMessageIds,
    onScrollStateChange,
    smartScroll,
    // NewChatComponent props
    selectedText,
    selectedAssistant,
    assistants,
    setSelectedAssistant,
}) => {
    if (conversationId) {
        return (
            <>
                <>
                    {virtualizeMessages
                        && scrollContainerRef
                        && clearPendingScrollMessageId
                        && setShiningMessageIds
                        && smartScroll ? (
                        <VirtualizedMessageList
                            allDisplayMessages={allDisplayMessages}
                            streamingMessages={streamingMessages}
                            shiningMessageIds={shiningMessageIds}
                            shiningMcpCallId={shiningMcpCallId}
                            reasoningExpandStates={reasoningExpandStates}
                            mcpToolCallStates={mcpToolCallStates}
                            generationGroups={generationGroups}
                            selectedVersions={selectedVersions}
                            getGenerationGroupControl={getGenerationGroupControl}
                            handleGenerationVersionChange={handleGenerationVersionChange}
                            onCodeRun={onCodeRun}
                            onMessageRegenerate={onMessageRegenerate}
                            onMessageEdit={onMessageEdit}
                            onMessageFork={onMessageFork}
                            onQueuedMessagePromote={onQueuedMessagePromote}
                            onToggleReasoningExpand={onToggleReasoningExpand}
                            inlineInteractionItems={inlineInteractionItems}
                            allowFeishuDebugResend={allowFeishuDebugResend}
                            scrollContainerRef={scrollContainerRef}
                            pendingScrollMessageId={pendingScrollMessageId}
                            clearPendingScrollMessageId={
                                clearPendingScrollMessageId
                            }
                            setShiningMessageIds={setShiningMessageIds}
                            onScrollStateChange={onScrollStateChange}
                            smartScroll={smartScroll}
                        />
                    ) : (
                        <MessageList
                            allDisplayMessages={allDisplayMessages}
                            streamingMessages={streamingMessages}
                            shiningMessageIds={shiningMessageIds}
                            shiningMcpCallId={shiningMcpCallId}
                            reasoningExpandStates={reasoningExpandStates}
                            mcpToolCallStates={mcpToolCallStates}
                            generationGroups={generationGroups}
                            selectedVersions={selectedVersions}
                            getGenerationGroupControl={getGenerationGroupControl}
                            handleGenerationVersionChange={
                                handleGenerationVersionChange
                            }
                            onCodeRun={onCodeRun}
                            onMessageRegenerate={onMessageRegenerate}
                            onMessageEdit={onMessageEdit}
                            onMessageFork={onMessageFork}
                            onQueuedMessagePromote={onQueuedMessagePromote}
                            onToggleReasoningExpand={onToggleReasoningExpand}
                            inlineInteractionItems={inlineInteractionItems}
                            allowFeishuDebugResend={allowFeishuDebugResend}
                        />
                    )}
                </>
            </>
        );
    }

    return (
        <NewChatComponent
            selectedText={selectedText}
            selectedAssistant={selectedAssistant}
            assistants={assistants}
            setSelectedAssistant={setSelectedAssistant}
        />
    );
});

ConversationContent.displayName = "ConversationContent";

export default ConversationContent;
