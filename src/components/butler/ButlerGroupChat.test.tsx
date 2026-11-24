import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ButlerGroupDetail } from "@/data/ButlerGroup";
import { AntiLeakageProvider } from "@/contexts/AntiLeakageContext";
import { clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import ButlerGroupChat from "./ButlerGroupChat";

// 群聊用的是通用输入框 InputArea：光标坐标依赖真实排版，测试环境里固定掉。
vi.mock("@/utils/caretCoordinates", () => ({ getCaretCoordinates: () => ({ cursorLeft: 0, cursorTop: 0 }) }));
// 群聊记录走与私聊相同的消息列表。显示设置打开“合并助手消息”，用来确认群里不同成员的发言不会被并成一个气泡。
vi.mock("@/hooks/useDisplayConfig", () => ({
    useDisplayConfig: () => ({
        config: {}, isLoading: false, error: null, isUserMessageMarkdownEnabled: false,
        isMergeAssistantMessages: true, isShowThinking: false, isPreviewCodeShowToolbar: false, refreshConfig: () => {},
    }),
}));
// 列表的跳转高亮用 ShineBorder 呈现，换成可查询的标记。
vi.mock("@/components/magicui/shine-border", () => ({ ShineBorder: () => <span data-testid="shine" /> }));

class ResizeObserverMock {
    observe() {}
    unobserve() {}
    disconnect() {}
}

const api = vi.hoisted(() => ({
    markRepliesRead: vi.fn().mockResolvedValue(undefined), getButlerGroup: vi.fn(), listOlderButlerGroupMessages: vi.fn(), postButlerGroupMessage: vi.fn(),
    stopButlerGroupReplies: vi.fn(), addButlerGroupMembers: vi.fn(), removeButlerGroupMember: vi.fn(),
    renameButlerGroup: vi.fn(), deleteButlerGroup: vi.fn(), onButlerGroupChanged: vi.fn(),
}));
vi.mock("@/services/butlerGroupService", () => api);
vi.mock("@/services/butlerMemoryService", () => ({
    listButlerMemories: vi.fn().mockResolvedValue([]), updateButlerMemory: vi.fn(), deleteButlerMemory: vi.fn(),
    getButlerMemorySource: vi.fn(), onButlerMemoryChanged: vi.fn().mockResolvedValue(() => {}),
}));
const askUser = vi.hoisted(() => vi.fn((_: { conversationId: number }) => ({ isDialogOpen: false } as Record<string, unknown>)));
vi.mock("@/hooks/useInlineInteraction", async importOriginal => ({ ...await importOriginal<typeof import("@/hooks/useInlineInteraction")>(), useAskUserQuestion: askUser }));
vi.mock("@/components/InlineInteractionCards", async importOriginal => ({
    ...await importOriginal<typeof import("@/components/InlineInteractionCards")>(),
    AskUserQuestionCard: ({ request }: { request: { questions: { question: string }[] } }) => <div>{request.questions[0].question}</div>,
}));
const elicitation = vi.hoisted(() => vi.fn((_: { conversationId: number }) => ({ questionRequest: null, decisionError: null })));
vi.mock("@/hooks/useAcpElicitation", () => ({ useAcpElicitation: elicitation }));
vi.mock("@/components/UnifiedMarkdown", () => ({ default: ({ children }: { children: string }) => <div>{children}</div> }));

const members = [
    { id: 1, assistant_id: 11, name: "总管家", conversation_id: 101, is_chief: true },
    { id: 2, assistant_id: 12, name: "写作助手", conversation_id: 102, is_chief: false },
];

function detail(patch: Partial<ButlerGroupDetail> = {}): ButlerGroupDetail {
    return {
        id: 7, name: "周报小组", members, has_more: false, turns: [], routing: false,
        interaction_turns: [{ conversation_id: 102, input_id: 8999, trigger_seq: 2, reply_seq: 3 }],
        messages: [
            { group_id: 7, seq: 1, sender_member: null, sender_assistant: null, sender_name: "系统", kind: "notice", body: "你邀请 总管家、写作助手 加入了群聊", reply_to: null, created_at: "2026-11-09 08:00:00", source_conversation_id: null, source_message_id: null },
            { group_id: 7, seq: 2, sender_member: null, sender_assistant: null, sender_name: "我", kind: "message", body: "帮我写周报", reply_to: null, created_at: "2026-11-09 08:01:00", source_conversation_id: null, source_message_id: null },
            { group_id: 7, seq: 3, sender_member: 2, sender_assistant: 12, sender_name: "写作助手", kind: "message", body: "初稿如下", reply_to: null, created_at: "2026-11-09 08:02:00", source_conversation_id: 102, source_message_id: 9001 },
        ],
        ...patch,
    };
}

function renderChat() {
    return render(<AntiLeakageProvider enabled={false}>
        <ButlerGroupChat groupId={7} chiefName="总管家" assistants={[{ id: 12, name: "写作助手", assistant_type: 0 }]} onDeleted={() => {}} />
    </AntiLeakageProvider>);
}

function memoryStorage() {
    const items = new Map<string, string>();
    return {
        getItem: (key: string) => items.get(key) ?? null,
        setItem: (key: string, value: string) => void items.set(key, value),
        removeItem: (key: string) => void items.delete(key),
        clear: () => items.clear(),
    };
}

// 先卸载组件再恢复全局对象，避免卸载前的异步刷新写入未替换的 localStorage。
afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
});

beforeEach(() => {
    vi.clearAllMocks();
    clearAllMockHandlers();
    // 通用输入框加载的 ! / # / 斜杠 / 全部助手 补全源；群聊的 @ 只用本群成员，不能出现全部助手里的人。
    mockInvokeHandler("get_mcp_tool_calls_by_conversation", () => []);
    mockInvokeHandler("list_butler_interactions", () => []);
    mockInvokeHandler("get_bang_list", () => []);
    mockInvokeHandler("vault_list_use_requests", () => []);
    mockInvokeHandler("vault_list_input_requests", () => []);
    mockInvokeHandler("get_butler_progress_summary", () => null);
    mockInvokeHandler("get_assistants", () => [{ id: 99, name: "写作外包" }]);
    mockInvokeHandler("get_artifacts_for_completion", () => []);
    mockInvokeHandler("get_skills_for_slash_completion", () => []);
    askUser.mockImplementation(() => ({ isDialogOpen: false }));
    vi.stubGlobal("localStorage", memoryStorage());
    vi.stubGlobal("ResizeObserver", ResizeObserverMock);
    const posted: ButlerGroupDetail["messages"] = [];
    api.getButlerGroup.mockImplementation(async () => {
        const current = detail();
        return { ...current, messages: [...current.messages, ...posted] };
    });
    api.onButlerGroupChanged.mockResolvedValue(() => {});
    api.postButlerGroupMessage.mockImplementation(async (_group: number, request_key: string, body: string, replyTo: number | null) => {
        const message = { request_key, group_id: 7, seq: 4 + posted.length, sender_member: null, sender_assistant: null, sender_name: "我",
            kind: "message" as const, body, reply_to: replyTo, created_at: "2026-11-09 08:03:00", source_conversation_id: null, source_message_id: null };
        posted.push(message);
        return message;
    });
    api.stopButlerGroupReplies.mockResolvedValue({ stopped: 1, errors: [] });
});

describe("ButlerGroupChat", () => {
    it("should show a plain chat stream without routing controls when opened", async () => {
        const focus = vi.spyOn(document, "hasFocus").mockReturnValue(true);
        renderChat();
        expect(await screen.findByText("初稿如下")).toBeInTheDocument();
        expect(screen.getByText("帮我写周报").closest("[data-aipp-slot='butler-group-user-message']")).not.toBeNull();
        expect(screen.getByText("初稿如下").closest("[data-aipp-slot='butler-group-member-message']")).not.toBeNull();
        expect(screen.getByText("你邀请 总管家、写作助手 加入了群聊")).toBeInTheDocument();
        // 没有选择接收人、顺序讨论之类的控件
        expect(screen.queryByText(/顺序征求意见|各自独立判断|仅引用资料/)).toBeNull();
        expect(screen.getByText("2 位成员")).toBeInTheDocument();
        await waitFor(() => expect(api.markRepliesRead).toHaveBeenCalledWith(7, 3, true, [3]));
        focus.mockRestore();
    });

    it("should send the typed text with a stable request key when pressing Enter", async () => {
        renderChat();
        const input = await screen.findByLabelText("群聊消息");
        fireEvent.change(input, { target: { value: "再精简一点" } });
        fireEvent.keyDown(input, { key: "Enter" });
        await waitFor(() => expect(api.postButlerGroupMessage).toHaveBeenCalledWith(7, expect.any(String), "再精简一点", null));
        expect(await screen.findByText("再精简一点")).toBeInTheDocument();
        await waitFor(() => expect((input as HTMLTextAreaElement).value).toBe(""));
    });

    it("should confirm a pending group bubble in place and keep a later draft", async () => {
        let accept!: () => void;
        const persist = api.postButlerGroupMessage.getMockImplementation()!;
        api.postButlerGroupMessage.mockImplementationOnce(async (...args) => {
            await new Promise<void>(resolve => { accept = resolve; });
            return { ...await persist(...args), request_key: args[1] };
        });
        renderChat();
        const input = await screen.findByLabelText("群聊消息");
        fireEvent.change(input, { target: { value: "确认这一条" } });
        fireEvent.keyDown(input, { key: "Enter" });
        const row = screen.getByText("确认这一条").closest("[data-message-item]");
        expect(row).toHaveTextContent("发送中");
        fireEvent.change(input, { target: { value: "下一条" } });
        await act(async () => accept());
        await waitFor(() => expect(row).not.toHaveTextContent("发送中"));
        expect(screen.getByText("确认这一条").closest("[data-message-item]")).toBe(row);
        expect(input).toHaveValue("下一条");
    });

    it("should not send when Enter confirms an IME composition", async () => {
        renderChat();
        const input = await screen.findByLabelText("群聊消息");
        fireEvent.change(input, { target: { value: "zhoubao" } });
        fireEvent.keyDown(input, { key: "Enter", isComposing: true });
        expect(api.postButlerGroupMessage).not.toHaveBeenCalled();
    });

    it("should jump to and highlight the user message a member reply quotes", async () => {
        const base = detail();
        api.getButlerGroup.mockResolvedValue({ ...base, messages: [...base.messages, {
            group_id: 7, seq: 4, sender_member: 1, sender_assistant: 11, sender_name: "总管家", kind: "message", body: "周报我来汇总",
            reply_to: 2, created_at: "2026-11-09 08:04:00", source_conversation_id: 101, source_message_id: 9002,
        }] });
        const scrolled = vi.fn();
        const original = HTMLElement.prototype.scrollIntoView;
        HTMLElement.prototype.scrollIntoView = function (this: HTMLElement) { scrolled(this.dataset.messageId); };
        try {
            renderChat();
            const reply = await screen.findByText("周报我来汇总");
            const label = within(reply.closest("[data-aipp-slot='butler-group-member-message']") as HTMLElement)
                .getByTitle("查看被引用的消息");
            expect(label).toHaveTextContent("我：帮我写周报");
            fireEvent.click(label);
            // 走与私聊相同的跳转链路：滚到被引用的那条（用户发言在列表里的 id 是 -序号）并闪烁高亮
            await waitFor(() => expect(scrolled).toHaveBeenCalledWith("-2"));
            const quotedRow = screen.getByText("帮我写周报").closest("[data-aipp-slot='butler-group-user-message']") as HTMLElement;
            await waitFor(() => expect(within(quotedRow).getByTestId("shine")).toBeInTheDocument());
        } finally {
            HTMLElement.prototype.scrollIntoView = original;
        }
    });

    it("should keep each member reply in its own bubble when merge display is on", async () => {
        const base = detail();
        api.getButlerGroup.mockResolvedValue({ ...base, messages: [...base.messages, {
            group_id: 7, seq: 4, sender_member: 1, sender_assistant: 11, sender_name: "总管家", kind: "message", body: "我补充数据",
            reply_to: null, created_at: "2026-11-09 08:02:30", source_conversation_id: 101, source_message_id: 9003,
        }] });
        renderChat();
        await screen.findByText("我补充数据");
        const rows = document.querySelectorAll("[data-aipp-slot='butler-group-member-message']");
        expect(rows).toHaveLength(2);
        expect(within(rows[0] as HTMLElement).getByText("写作助手")).toBeInTheDocument();
        expect(within(rows[1] as HTMLElement).getByText("总管家")).toBeInTheDocument();
    });

    it("should load the whole history at once when the group has older messages", async () => {
        const base = detail({ has_more: true });
        api.getButlerGroup.mockResolvedValue(base);
        api.listOlderButlerGroupMessages.mockResolvedValue([
            { group_id: 7, seq: 0, sender_member: null, sender_assistant: null, sender_name: "我", kind: "message", body: "很早以前的话",
                reply_to: null, created_at: "2026-11-08 08:00:00", source_conversation_id: null, source_message_id: null },
        ]);
        renderChat();
        expect(await screen.findByText("很早以前的话")).toBeInTheDocument();
        expect(api.listOlderButlerGroupMessages).toHaveBeenCalledWith(7, 1, 200);
        expect(screen.queryByText("查看更早的消息")).toBeNull();
    });

    it("should post a continuation when clicking send in an idle group with an empty draft", async () => {
        renderChat();
        await screen.findByText("初稿如下");
        fireEvent.click(screen.getByLabelText("发送"));
        await waitFor(() => expect(api.postButlerGroupMessage).toHaveBeenCalledWith(7, expect.any(String), "继续你的工作", null));
    });

    it("should page past 200 older messages and keep the loaded history when the group refreshes", async () => {
        const base = detail();
        const make = (seq: number) => ({ ...base.messages[0], seq, body: `历史 ${seq}` });
        api.getButlerGroup.mockResolvedValueOnce(detail({messages:[make(204)], has_more:true}))
            .mockResolvedValue(detail({messages:[make(205)], has_more:true}));
        api.listOlderButlerGroupMessages.mockImplementation(async (_id, before, limit) => {
            const first = Math.max(1, before - limit);
            return Array.from({length:before - first}, (_, i) => make(first + i));
        });
        renderChat();
        await waitFor(() => expect(api.listOlderButlerGroupMessages.mock.calls).toEqual([[7,204,200], [7,4,200]]));
        const refresh = api.onButlerGroupChanged.mock.calls[0][0];
        refresh(7);
        await waitFor(() => expect(api.getButlerGroup).toHaveBeenCalledTimes(2));
        expect(api.listOlderButlerGroupMessages).toHaveBeenCalledTimes(2);
    });

    it("should quote a member reply with the private chat quote button and chip", async () => {
        renderChat();
        const reply = await screen.findByText("初稿如下");
        fireEvent.click(within(reply.closest("[data-aipp-slot='butler-group-member-message']") as HTMLElement).getByLabelText("引用回复"));
        expect(document.querySelector('[data-aipp-slot="butler-quote-chip"]')).toHaveTextContent("写作助手初稿如下");
        fireEvent.change(screen.getByLabelText("群聊消息"), { target: { value: "第二段改短" } });
        fireEvent.click(screen.getByLabelText("发送"));
        await waitFor(() => expect(api.postButlerGroupMessage).toHaveBeenCalledWith(7, expect.any(String), "第二段改短", 3));
        // 发出去的消息带着私聊同款的引用标签
        await waitFor(() => expect(document.querySelector('[data-aipp-slot="butler-message-quote"]')).toHaveTextContent("写作助手：初稿如下"));
        expect(document.querySelector('[data-aipp-slot="butler-quote-chip"]')).toBeNull();
    });

    it("should insert a member name when choosing an @ suggestion", async () => {
        renderChat();
        const input = await screen.findByLabelText("群聊消息") as HTMLTextAreaElement;
        fireEvent.change(input, { target: { value: "@写", selectionStart: 2 } });
        const option = await screen.findByRole("option", { name: /写作助手/ });
        // 候选只来自本群成员，不是全部助手
        expect(screen.queryByRole("option", { name: /写作外包/ })).toBeNull();
        fireEvent.click(option);
        expect(input.value).toBe("@写作助手 ");
    });

    it("should offer everyone in @ suggestions when the group has several members", async () => {
        renderChat();
        const input = await screen.findByLabelText("群聊消息");
        fireEvent.change(input, { target: { value: "@", selectionStart: 1 } });
        const list = await screen.findByRole("listbox");
        const names = within(list).getAllByRole("option")
            .map(option => option.querySelector(".assistant-completion-name")?.textContent);
        expect(names).toEqual(["总管家", "写作助手", "所有人"]);
    });

    it("should not offer attachments when chatting in a group", async () => {
        renderChat();
        await screen.findByLabelText("群聊消息");
        expect(screen.queryByLabelText("添加附件")).toBeNull();
    });

    it("should show who is typing and offer stop when members are replying", async () => {
        api.getButlerGroup.mockResolvedValue(detail({ turns: [
            { member_id: 2, state: "running", round: 1, rounds: 1 }, { member_id: 1, state: "running", round: 1, rounds: 1 },
        ] }));
        renderChat();
        // 两位成员同时回复
        expect(await screen.findByRole("status", { name: "写作助手 正在输入" })).toBeInTheDocument();
        expect(await screen.findByRole("status", { name: "总管家 正在输入" })).toBeInTheDocument();
        expect(screen.getByText("总管家、写作助手 正在输入…")).toBeInTheDocument();
        // 与私聊同一个发送键：输入为空时是停止
        fireEvent.click(screen.getByLabelText("停止"));
        await waitFor(() => expect(api.stopButlerGroupReplies).toHaveBeenCalledWith(7));
    });

    it("should send instead of stopping when typing while members are replying", async () => {
        api.getButlerGroup.mockResolvedValue(detail({ turns: [{ member_id: 2, state: "running", round: 1, rounds: 1 }] }));
        renderChat();
        const input = await screen.findByLabelText("群聊消息");
        await screen.findByRole("status", { name: "写作助手 正在输入" });
        fireEvent.change(input, { target: { value: "先停一下，改成三段" } });
        fireEvent.keyDown(input, { key: "Enter" });
        await waitFor(() => expect(api.postButlerGroupMessage).toHaveBeenCalledWith(7, expect.any(String), "先停一下，改成三段", null));
        expect(api.stopButlerGroupReplies).not.toHaveBeenCalled();
    });

    it("should restore the reporting member's words when opening an active group", async () => {
        api.getButlerGroup.mockResolvedValue(detail({ turns: [
            { member_id: 1, state: "running", round: 1, rounds: 1 },
            { member_id: 2, state: "running", round: 1, rounds: 1 },
        ] }));
        mockInvokeHandler("get_butler_progress_summary", args => args?.conversationId === 102
            ? { conversation_id: 102, epoch: 1, reports: ["我先整理本周的交付。"], helper: { text: "正在核对交付记录", steps: [] } }
            : { conversation_id: 101, epoch: 1, reports: [], helper: { text: "正在查看任务安排", steps: [] } });
        renderChat();
        const writer = await screen.findByRole("status", { name: "写作助手 正在输入" });
        expect(await within(writer).findByText("我先整理本周的交付。")).toBeInTheDocument();
        expect(screen.getAllByText("我先整理本周的交付。")).toHaveLength(1);
        const chief = screen.getByRole("status", { name: "总管家 正在输入" });
        for (const [member, name, text] of [[writer, "写作助手", "正在核对交付记录"], [chief, "总管家", "正在查看任务安排"]] as const) {
            const summary = await within(member).findByRole("status");
            expect(summary).toHaveClass("justify-start");
            expect(summary).toHaveTextContent(`${name}·${text}`);
        }
    });

    it("should show discussion progress when members are discussing in rounds", async () => {
        api.getButlerGroup.mockResolvedValue(detail({ turns: [{ member_id: 2, state: "queued", round: 2, rounds: 3 }] }));
        renderChat();
        expect(await screen.findByText("讨论中 · 第 2/3 轮")).toBeInTheDocument();
    });

    it("should display a group member's file result next to their reply", async () => {
        const call = { id: 121, conversation_id: 102, message_id: 9000, status: "success", tool_name: "preview_file",
            parameters: JSON.stringify({ files: [{ type: "text", title: "初稿.txt", content: "群里的文件内容" }] }) };
        mockInvokeHandler("get_mcp_tool_calls_by_conversation", args => args?.conversationId === 102 ? [call] : []);
        mockInvokeHandler("get_mcp_tool_call", () => call);
        mockInvokeHandler("prepare_preview_file_request_for_ui", args => args?.request);
        renderChat();
        const draft = await screen.findByText("群里的文件内容");
        expect(draft.closest('[data-message-type="ui_interaction_preview_file"]')).toBeTruthy();
        expect(draft.compareDocumentPosition(screen.getByText("初稿如下")) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
        expect(screen.queryByText("preview_file")).toBeNull();
    });

    it("should keep a member's open questionnaire between their report and another member's reply", async () => {
        const base = detail();
        const reply = { ...base.messages[2], sender_member:1, sender_assistant:11, sender_name:"总管家", source_conversation_id:101 };
        api.getButlerGroup.mockResolvedValue(detail({messages:[...base.messages.slice(0, 2), reply],
            interaction_turns:[{conversation_id:102,input_id:8900,trigger_seq:2,reply_seq:null}]}));
        const at = Date.parse("2026-11-09T08:01:20Z");
        mockInvokeHandler("list_butler_interactions", args => args?.conversationId === 102 ? [
            { kind:"report", request_id:"r", id:1, anchor:9000, occurred_ms:at, request:{ text:"正在核对" } },
            { kind:"question", request_id:"q", id:2, anchor:9000, occurred_ms:at+1,
                request:{ request_id:"q", questions:[{question:"要哪一版？"}] }, answers:null },
        ] : []);
        askUser.mockImplementation(({conversationId}) => conversationId === 102
            ? { isDialogOpen:true, pendingRequest:{request_id:"q", questions:[{question:"要哪一版？"}]}, viewMode:"questionnaire", messageId:9000 }
            : { isDialogOpen:false });
        renderChat();
        const report = await screen.findByText("正在核对");
        const question = screen.getByText("要哪一版？");
        expect(report.compareDocumentPosition(question) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
        expect(question.compareDocumentPosition(screen.getByText("初稿如下")) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
        expect(screen.getAllByText("要哪一版？")).toHaveLength(1);
    });

    it("should show a member question while other members keep replying after @everyone", async () => {
        api.getButlerGroup.mockResolvedValue(detail({ turns: [
            { member_id: 1, state: "running", round: 1, rounds: 1 }, { member_id: 2, state: "running", round: 1, rounds: 1 },
        ] }));
        askUser.mockImplementation(({ conversationId }) => conversationId === 102
            ? { isDialogOpen: true, pendingRequest: { request_id: "q1", questions: [{ question: "周报发给谁？" }] }, viewMode: "questionnaire" }
            : { isDialogOpen: false });
        renderChat();
        expect(await screen.findByText("写作助手 想问你")).toBeInTheDocument();
        expect(screen.getByText("周报发给谁？")).toBeInTheDocument();
        expect(await screen.findByRole("status", { name: "总管家 正在输入" })).toBeInTheDocument();
        expect(screen.queryByText("总管家 想问你")).toBeNull();
    });

    it("should listen for native questions from every member execution", async () => {
        renderChat();
        await screen.findByText("初稿如下");
        expect(elicitation).toHaveBeenCalledWith({ conversationId: 101 });
        expect(elicitation).toHaveBeenCalledWith({ conversationId: 102 });
    });

    it("should retain progress sent before a stopped turn and discard its late events", async () => {
        const base = detail();
        api.getButlerGroup.mockResolvedValue(detail({messages:base.messages.slice(0, 2),
            interaction_turns:[{conversation_id:102,input_id:8999,trigger_seq:2,reply_seq:null,ended_at:"2026-11-09 08:01:20.500"}]}));
        const before = Date.parse("2026-11-09T08:01:20.200Z");
        mockInvokeHandler("list_butler_interactions", args => args?.conversationId === 102 ? [
            {kind:"report", id:1, request_id:"r1", anchor:9000, occurred_ms:before, request:{text:"停止前的进度"}},
            {kind:"report", id:2, request_id:"r2", anchor:9000, occurred_ms:before+800, request:{text:"迟到的进度"}},
        ] : []);
        renderChat();
        expect(await screen.findByText("停止前的进度")).toBeInTheDocument();
        expect(screen.queryByText("迟到的进度")).toBeNull();
    });
});


describe("group Vault card placement", () => {
    it("should pin cards to their messages, wait for unloaded targets, and remove resolved cards", async () => {
        const { listen } = await import("@tauri-apps/api/event");
        const { default: Transcript } = await import("./ButlerGroupTranscript");
        const original = vi.mocked(listen).getMockImplementation();
        const handlers = new Map<string, (event: { payload: unknown }) => void>();
        vi.mocked(listen).mockImplementation(async (name, handler) => {
            handlers.set(name, handler as (event: { payload: unknown }) => void);
            return () => { handlers.delete(name); };
        });
        const use = { requestId: "use-317", conversationId: 102, messageId: 9999, groupAnchor: { groupId: 7, seq: 2 },
            assistantName: "写作助手", entries: [{ id: "synthetic", key: "fake_key", name: "测试凭据" }], tool: "synthetic",
            destination: { kind: "local", label: "本机测试", origin: null, insecure: false }, preview: "<vault:fake_key>",
            previewKind: "command", canAlways: false, createdAt: 1 };
        const input = { requestId: "input-317", conversationId: 101, messageId: 9998, groupAnchor: { groupId: 7, seq: 3 },
            assistantName: "总管家", purpose: "合成测试", fields: [{ key: "other_key", name: "测试填写", description: "" }], createdAt: 2 };
        mockInvokeHandler("vault_list_use_requests", () => [use, use]);
        mockInvokeHandler("vault_list_input_requests", () => [input]);
        mockInvokeHandler("list_butler_interactions", args => args?.conversationId === 102 ? [{
            kind: "report", id: 317, request_id: "report-317", anchor: 9000,
            occurred_ms: Date.parse("2026-11-09T08:01:20Z"), request: { text: "凭据请求期间的进度" },
        }] : []);
        const base = detail().messages;
        const view = (messages: ButlerGroupDetail["messages"]) => <AntiLeakageProvider enabled={false}>
            <Transcript groupId={7} historyLoaded messages={messages} members={members} running={[]} turns={detail().interaction_turns} />
        </AntiLeakageProvider>;
        try {
            const mounted = render(view(base.filter(message => message.seq !== 2)));
            await waitFor(() => expect(document.querySelector('[data-vault-request-id="input-317"]')).not.toBeNull());
            expect(document.querySelector('[data-vault-request-id="use-317"]')).toBeNull();
            expect(document.querySelector('[data-aipp-slot="butler-group-vault-pending"]')).toHaveTextContent("1 个凭据请求");
            mounted.rerender(view(base));
            await waitFor(() => expect(document.querySelector('[data-vault-request-id="use-317"]')).not.toBeNull());
            const anchored = (id: string) => document.querySelector(`[data-vault-request-id="${id}"]`)?.closest("[data-group-message-seq]")?.getAttribute("data-group-message-seq");
            expect(anchored("use-317")).toBe("2");
            expect(anchored("input-317")).toBe("3");
            const report = await screen.findByText("凭据请求期间的进度");
            expect(report.compareDocumentPosition(screen.getByText("初稿如下")) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
            expect(screen.getAllByText("凭据请求期间的进度")).toHaveLength(1);
            expect(document.querySelectorAll('[data-vault-request-id="use-317"]')).toHaveLength(1);
            mounted.rerender(view([...base, { ...base[2], seq: 4, source_message_id: 9002, body: "新的成员消息" }]));
            expect(anchored("use-317")).toBe("2");
            expect(anchored("input-317")).toBe("3");
            act(() => {
                handlers.get("vault-use-resolved")?.({ payload: { requestId: use.requestId, conversationId: 102 } });
                handlers.get("vault-input-resolved")?.({ payload: { requestId: input.requestId, conversationId: 101 } });
            });
            await waitFor(() => expect(document.querySelectorAll('[data-aipp-slot="butler-group-vault"]')).toHaveLength(0));
            act(() => handlers.get("vault-use-request")?.({ payload: use }));
            expect(document.querySelectorAll('[data-aipp-slot="butler-group-vault"]')).toHaveLength(0);
            mounted.unmount();
        } finally {
            if (original) vi.mocked(listen).mockImplementation(original);
        }
    });

    it("should mark the empty side columns of the group header as drag regions on Windows", async () => {
        const { __resetPlatformCacheForTests } = await import("@/hooks/use-platform");
        __resetPlatformCacheForTests();
        mockInvokeHandler("get_platform", () => "windows");
        renderChat();

        const header = await waitFor(() => {
            const el = document.querySelector('[data-aipp-slot="butler-group-header"]') as HTMLElement | null;
            expect(el).toBeTruthy();
            expect(el).toHaveAttribute("data-tauri-drag-region");
            return el!;
        });
        const [leftSpacer, centerButton, rightColumn] = Array.from(header.children) as HTMLElement[];
        expect(leftSpacer).toHaveAttribute("data-tauri-drag-region");
        expect(centerButton).not.toHaveAttribute("data-tauri-drag-region");
        expect(rightColumn).toHaveAttribute("data-tauri-drag-region");
        expect(screen.getByTitle("群成员")).not.toHaveAttribute("data-tauri-drag-region");
    });
});
