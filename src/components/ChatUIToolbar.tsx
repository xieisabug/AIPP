import { Button } from "./ui/button";
import { memo } from "react";

interface ChatUIToolbarProps {
    onNewConversation: () => void;
    onSearch: () => void;
}

const ChatUIToolbar = memo(function ChatUIToolbar({ onNewConversation, onSearch }: ChatUIToolbarProps) {
    return (
        <div className="flex flex-none h-20 items-center justify-center pt-3 bg-background rounded-t-xl" data-aipp-slot="chat-toolbar">
            <Button className="w-24 select-none" onClick={onSearch} data-aipp-slot="chat-toolbar-search">
                搜索
            </Button>
            <Button
                className="w-24 ml-4 select-none"
                onClick={onNewConversation}
                data-aipp-slot="chat-toolbar-new-conversation"
            >
                新对话
            </Button>
        </div>
    );
});

ChatUIToolbar.displayName = "ChatUIToolbar";

export default ChatUIToolbar;
