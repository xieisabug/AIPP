import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ButlerGroupDetail } from "@/data/ButlerGroup";
import { AntiLeakageProvider } from "@/contexts/AntiLeakageContext";
import ButlerGroupChat from "./ButlerGroupChat";

const api = vi.hoisted(() => ({
    getButlerGroup: vi.fn(), listOlderButlerGroupMessages: vi.fn(), postButlerGroupMessage: vi.fn(),
    stopButlerGroupReplies: vi.fn(), addButlerGroupMembers: vi.fn(), removeButlerGroupMember: vi.fn(),
    renameButlerGroup: vi.fn(), deleteButlerGroup: vi.fn(), onButlerGroupChanged: vi.fn(),
}));
vi.mock("@/services/butlerGroupService", () => api);
const askUser = vi.hoisted(() => vi.fn((_: { conversationId: number }) => ({ isDialogOpen: false } as Record<string, unknown>)));
vi.mock("@/hooks/useInlineInteraction", () => ({ useAskUserQuestion: askUser }));
vi.mock("@/components/InlineInteractionCards", () => ({
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
    askUser.mockImplementation(() => ({ isDialogOpen: false }));
    vi.stubGlobal("localStorage", memoryStorage());
    const posted: ButlerGroupDetail["messages"] = [];
    api.getButlerGroup.mockImplementation(async () => {
        const current = detail();
        return { ...current, messages: [...current.messages, ...posted] };
    });
    api.onButlerGroupChanged.mockResolvedValue(() => {});
    api.postButlerGroupMessage.mockImplementation(async (_group: number, _key: string, body: string, replyTo: number | null) => {
        const message = { group_id: 7, seq: 4 + posted.length, sender_member: null, sender_assistant: null, sender_name: "我",
            kind: "message" as const, body, reply_to: replyTo, created_at: "2026-11-09 08:03:00", source_conversation_id: null, source_message_id: null };
        posted.push(message);
        return message;
    });
    api.stopButlerGroupReplies.mockResolvedValue({ stopped: 1, errors: [] });
});

describe("ButlerGroupChat", () => {
    it("should show a plain chat stream without routing controls when opened", async () => {
        renderChat();
        expect(await screen.findByText("初稿如下")).toBeInTheDocument();
        expect(screen.getByText("帮我写周报").closest("[data-aipp-slot='butler-group-user-message']")).not.toBeNull();
        expect(screen.getByText("初稿如下").closest("[data-aipp-slot='butler-group-member-message']")).not.toBeNull();
        expect(screen.getByText("你邀请 总管家、写作助手 加入了群聊")).toBeInTheDocument();
        // 没有选择接收人、顺序讨论之类的控件
        expect(screen.queryByText(/顺序征求意见|各自独立判断|仅引用资料/)).toBeNull();
        expect(screen.getByText("2 位成员")).toBeInTheDocument();
        expect(localStorage.getItem("butler-group-read-7")).toBe("3");
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

    it("should not send when Enter confirms an IME composition", async () => {
        renderChat();
        const input = await screen.findByLabelText("群聊消息");
        fireEvent.change(input, { target: { value: "zhoubao" } });
        fireEvent.keyDown(input, { key: "Enter", isComposing: true });
        expect(api.postButlerGroupMessage).not.toHaveBeenCalled();
    });

    it("should jump to the user message a member reply quotes", async () => {
        const base = detail();
        api.getButlerGroup.mockResolvedValue({ ...base, messages: [...base.messages, {
            group_id: 7, seq: 4, sender_member: 1, sender_assistant: 11, sender_name: "总管家", kind: "message", body: "周报我来汇总",
            reply_to: 2, created_at: "2026-11-09 08:04:00", source_conversation_id: 101, source_message_id: 9002,
        }] });
        const scrolled = vi.fn();
        const original = HTMLElement.prototype.scrollIntoView;
        HTMLElement.prototype.scrollIntoView = function (this: HTMLElement) { scrolled(this.dataset.aippGroupSeq); };
        try {
            renderChat();
            const reply = await screen.findByText("周报我来汇总");
            const label = within(reply.closest("[data-aipp-slot='butler-group-member-message']") as HTMLElement)
                .getByTitle("查看被引用的消息");
            expect(label).toHaveTextContent("我：帮我写周报");
            fireEvent.click(label);
            expect(scrolled).toHaveBeenCalledWith("2");
        } finally {
            HTMLElement.prototype.scrollIntoView = original;
        }
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
        fireEvent.mouseDown(option);
        expect(input.value).toBe("@写作助手 ");
    });

    it("should show who is typing and offer stop when members are replying", async () => {
        api.getButlerGroup.mockResolvedValue(detail({ turns: [
            { member_id: 2, state: "running", round: 1, rounds: 1 }, { member_id: 1, state: "running", round: 1, rounds: 1 },
        ] }));
        renderChat();
        // 两位成员同时回复
        expect(await screen.findByRole("status", { name: "写作助手 正在输入" })).toBeInTheDocument();
        expect(screen.getByRole("status", { name: "总管家 正在输入" })).toBeInTheDocument();
        expect(screen.getByText("总管家、写作助手 正在输入…")).toBeInTheDocument();
        fireEvent.click(screen.getByLabelText("停止回复"));
        await waitFor(() => expect(api.stopButlerGroupReplies).toHaveBeenCalledWith(7));
    });

    it("should show discussion progress when members are discussing in rounds", async () => {
        api.getButlerGroup.mockResolvedValue(detail({ turns: [{ member_id: 2, state: "queued", round: 2, rounds: 3 }] }));
        renderChat();
        expect(await screen.findByText("讨论中 · 第 2/3 轮")).toBeInTheDocument();
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
        expect(screen.getByRole("status", { name: "总管家 正在输入" })).toBeInTheDocument();
        expect(screen.queryByText("总管家 想问你")).toBeNull();
    });

    it("should listen for native questions from every member execution", async () => {
        renderChat();
        await screen.findByText("初稿如下");
        expect(elicitation).toHaveBeenCalledWith({ conversationId: 101 });
        expect(elicitation).toHaveBeenCalledWith({ conversationId: 102 });
    });
});
