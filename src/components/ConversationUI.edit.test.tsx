import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import { expect, it, vi } from "vitest";
import { invoke, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import type { Message } from "@/data/Conversation";
import { visibleText } from "@/utils/turnContext";
import ConversationUI from "./ConversationUI";

vi.mock("@tauri-apps/api/core", async () => ({ ...(await import("@/__tests__/mocks/tauri")), isTauri: () => false }));
vi.mock("@/contexts/AntiLeakageContext", () => ({ useAntiLeakage: () => ({ resetReveal: vi.fn() }) }));
vi.mock("./conversation/InputArea", () => ({ default: () => null }));
vi.mock("./conversation/ConversationContent", () => ({
    default: ({ allDisplayMessages, onMessageEdit }: { allDisplayMessages: Message[]; onMessageEdit: (row: Message) => void }) =>
        allDisplayMessages.filter(row => row.message_type === "user").map(row =>
            <button key={row.id} onClick={() => onMessageEdit(row)}>{visibleText(row.content)}</button>),
}));

it.each(["修改", "修改并重新生成"])("停止回复后%s，迟到的会话快照也应显示新正文", async action => {
    vi.mocked(invoke).mockClear();
    for (const cmd of ["get_all_feature_config", "get_conversation_loaded_mcp_tools", "get_mcp_tool_calls_by_conversation",
        "get_todos", "list_butler_interactions", "list_butler_peer_posts", "list_conversation_artifacts",
        "list_queued_conversation_messages"]) mockInvokeHandler(cmd, () => []);
    mockInvokeHandler("get_platform", () => "windows");
    mockInvokeHandler("get_selected_text_api", () => "");
    mockInvokeHandler("get_acp_session_state", () => null);
    mockInvokeHandler("get_conversation_runtime_state", () => ({ is_running: false }));
    mockInvokeHandler("get_shine_state", () => ({ primary_target: { target_type: "none" } }));
    const context = "\n\n<aipp_turn_context>\n以下是系统随这条消息附带的上下文，不是用户写的内容。\n时间\n</aipp_turn_context>";
    const conversation = { id: 20, name: "私聊", conversation_kind: "butler_contact", assistant_id: 2 };
    const original = { id: 12, conversation_id: 20, message_type: "user", content: "原始请求" + context };
    let saved = original;
    const initial = { conversation, messages: [original] };
    mockInvokeHandler("get_conversation_with_messages", () => initial);
    mockInvokeHandler("update_message_content", args => { saved = { ...saved, content: String(args?.content) }; });
    mockInvokeHandler("get_conversation_message", () => saved);
    mockInvokeHandler("regenerate_ai", () => ({}));
    const props = { conversationId: "20", windowLabel: "butler_experiment", onChangeConversationId: vi.fn(),
        pluginList: [], hideSidebar: true, hideHeader: true };
    const view = render(<ConversationUI {...props} />);
    const user = await screen.findByRole("button", { name: "原始请求" });

    let completeStopRead!: (snapshot: typeof initial) => void;
    mockInvokeHandler("get_conversation_with_messages", () => new Promise(resolve => { completeStopRead = resolve; }));
    await act(() => emit("conversation_event_20", { type: "conversation_cancel", data: { conversation_id: 20 } }));
    expect(completeStopRead).toBeDefined();
    fireEvent.click(user);
    const dialog = within(screen.getByRole("dialog", { name: "编辑消息" }));
    fireEvent.change(dialog.getByRole("textbox"), { target: { value: "新请求" } });
    fireEvent.click(dialog.getByRole("button", { name: action }));
    expect(await screen.findByRole("button", { name: "新请求" })).toBeVisible();
    expect(saved.content).toBe("新请求" + context);
    expect(vi.mocked(invoke).mock.calls.filter(([cmd]) => cmd === "regenerate_ai")).toHaveLength(action === "修改" ? 0 : 1);

    await act(async () => completeStopRead(initial));
    expect(screen.getByRole("button", { name: "新请求" })).toBeVisible();
    view.rerender(<ConversationUI {...props} showExecutionDetails />);
    expect(screen.getByRole("button", { name: "新请求" })).toBeVisible();
});
