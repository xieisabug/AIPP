import React, { memo } from "react";
import ConversationTitle from "./ConversationTitle";
import { Conversation } from "../../data/Conversation";

export interface ConversationHeaderProps {
    conversationId: string;
    conversation?: Conversation;
    onEdit: () => void;
    onDelete: () => void;
    statsOpen?: boolean;
    onStatsOpenChange?: (open: boolean) => void;
    exportOpen?: boolean;
    onExportOpenChange?: (open: boolean) => void;
    allowRename?: boolean;
    allowDelete?: boolean;
    extraActions?: React.ReactNode;
}

const ConversationHeader: React.FC<ConversationHeaderProps> = memo(({
    conversationId,
    conversation,
    onEdit,
    onDelete,
    statsOpen,
    onStatsOpenChange,
    exportOpen,
    onExportOpenChange,
    allowRename = true,
    allowDelete = true,
    extraActions,
}) => {
    if (!conversationId) {
        return null;
    }

    return (
        <ConversationTitle
            onEdit={onEdit}
            onDelete={onDelete}
            conversation={conversation}
            statsOpen={statsOpen}
            onStatsOpenChange={onStatsOpenChange}
            exportOpen={exportOpen}
            onExportOpenChange={onExportOpenChange}
            allowRename={allowRename}
            allowDelete={allowDelete}
            extraActions={extraActions}
        />
    );
});

ConversationHeader.displayName = "ConversationHeader";

export default ConversationHeader;
