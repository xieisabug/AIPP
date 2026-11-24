import { Plus, Search } from "lucide-react";
import { memo } from "react";
import { Button } from "./ui/button";
import { cn } from "../utils/utils";

interface ChatUIToolbarProps {
    onNewConversation: () => void;
    onSearch: () => void;
    /**
     * - `row`：独立一行，两个按钮并排（桌面非 macOS、移动端侧栏）；
     * - `inline`：无边框紧凑按钮（与总管家侧栏图标一致），嵌在桌面侧栏标题栏行右侧，不单独占行。
     */
    layout?: "row" | "inline";
    /** 快捷键显示文本（如「⌘ N」），放在悬停提示里；不传则只显示名称（移动端没有键盘快捷键）。 */
    newConversationShortcut?: string;
    searchShortcut?: string;
}

const withShortcut = (label: string, shortcut?: string) => (shortcut ? `${label}（${shortcut}）` : label);

/** 侧栏主操作：搜索、新对话。 */
const ChatUIToolbar = memo(function ChatUIToolbar({
    onNewConversation,
    onSearch,
    layout = "row",
    newConversationShortcut,
    searchShortcut,
}: ChatUIToolbarProps) {
    const inline = layout === "inline";
    return (
        <div
            className={cn(
                "flex flex-none items-center",
                inline ? "ml-auto gap-1.5" : "gap-2 bg-background px-3 pb-2 pt-3 rounded-t-xl",
            )}
            data-tauri-drag-region={inline || undefined}
            data-aipp-slot="chat-toolbar"
        >
            <Button
                variant={inline ? "ghost" : "outline"}
                size={inline ? "icon" : "default"}
                className={cn("select-none", inline ? "size-8 rounded-full text-muted-foreground hover:text-foreground" : "h-9 flex-1 gap-2 font-normal")}
                aria-label="搜索"
                title={withShortcut("搜索", searchShortcut)}
                onClick={onSearch}
                data-aipp-slot="chat-toolbar-search"
            >
                <Search />
                {!inline && <span>搜索</span>}
            </Button>
            <Button
                variant={inline ? "ghost" : "outline"}
                className={cn("select-none gap-1.5 font-normal", inline ? "h-8 rounded-full px-2.5 text-muted-foreground hover:text-foreground" : "h-9 flex-1 gap-2")}
                title={withShortcut("新对话", newConversationShortcut)}
                onClick={onNewConversation}
                data-aipp-slot="chat-toolbar-new-conversation"
            >
                <Plus />
                <span>新对话</span>
            </Button>
        </div>
    );
});

ChatUIToolbar.displayName = "ChatUIToolbar";

export default ChatUIToolbar;
