import { act, fireEvent, render, renderHook, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { taskQuestionItems, useButlerTaskQuestions, type ButlerTaskQuestion } from "./butlerTaskQuestions";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn(() => Promise.resolve(vi.fn())) }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));
vi.mock("@/components/InlineInteractionCards", () => ({
    AskUserQuestionCard: ({ onSubmit, onCancel, readOnly, completedAnswers }: any) => readOnly ? <div>已答：{JSON.stringify(completedAnswers)}</div> : <div>
        <button onClick={() => onSubmit("r", { 格式: "Excel" })}>提交</button>
        <button onClick={() => onCancel("r")}>取消</button>
    </div>,
}));

const question = (extra: Partial<ButlerTaskQuestion>): ButlerTaskQuestion => ({
    task_conversation_id: 41, task_title: "周报", request_id: "req-1", call_id: null, stale: false,
    questions: [{ question: "用哪种格式？" } as any], ...extra,
});

describe("Butler task questions in the executor's chat", () => {
    it("should label which task asked and pass the answer back to that question", () => {
        const submit = vi.fn(); const cancel = vi.fn();
        const items = taskQuestionItems([question({}), question({ task_conversation_id: 42, task_title: "月报", request_id: null, call_id: 9, stale: true })], "写作", submit, cancel);
        expect(items.map(item => item.messageId)).toEqual([null, null]);
        render(<>{items.map(item => <div key={item.key}>{item.content}</div>)}</>);
        expect(screen.getByText(/写作 在任务《周报》中向你提问/)).toBeVisible();
        expect(screen.getByText(/月报.*原执行已中断/)).toBeVisible();
        fireEvent.click(screen.getAllByRole("button", { name: "提交" })[1]);
        expect(submit).toHaveBeenCalledWith(expect.objectContaining({ task_conversation_id: 42, call_id: 9 }), { 格式: "Excel" });
        fireEvent.click(screen.getAllByRole("button", { name: "取消" })[0]);
        expect(cancel).toHaveBeenCalledWith(expect.objectContaining({ request_id: "req-1" }));
    });
});


function deferred<T>() {
    let resolve!: (value: T) => void;
    const promise = new Promise<T>(done => { resolve = done; });
    return { promise, resolve };
}

describe("contact-bound question loading and replies", () => {
    beforeEach(() => { mocks.invoke.mockReset(); mocks.listen.mockClear(); });

    it("should ignore an older contact response and reject its captured submit when switching contacts", async () => {
        const old = deferred<ButlerTaskQuestion[]>(); const next = deferred<ButlerTaskQuestion[]>();
        mocks.invoke.mockImplementation((command: string, args: any) => command === "list_butler_contact_task_questions" ? (args.conversationId === 1 ? old.promise : next.promise) : Promise.resolve());
        const { result, rerender } = renderHook(({ contact }) => useButlerTaskQuestions(contact), { initialProps: { contact: 1 } });
        const capturedSubmit = result.current.submit;
        rerender({ contact: 2 });
        const newQuestion = question({ request_id: "req-2", task_conversation_id: 42 });
        await act(async () => next.resolve([newQuestion]));
        await act(async () => old.resolve([question({})]));
        expect(result.current.questions).toEqual([newQuestion]);
        await act(async () => capturedSubmit(question({}), { 格式: "PDF" }));
        expect(mocks.invoke.mock.calls.every(([command]) => command === "list_butler_contact_task_questions")).toBe(true);
        await act(async () => result.current.submit(newQuestion, { 格式: "PDF" }));
        expect(mocks.invoke).toHaveBeenCalledWith("respond_butler_contact_task_question", expect.objectContaining({ conversationId: 2, taskConversationId: 42, requestId: "req-2", cancelled: false }));
    });

    it("should clear old controls immediately and not cancel another contact's request", async () => {
        const first = question({}); const pending = deferred<ButlerTaskQuestion[]>();
        mocks.invoke.mockImplementation((_: string, args: any) => args.conversationId === 1 ? Promise.resolve([first]) : pending.promise);
        const { result, rerender } = renderHook(({ contact }) => useButlerTaskQuestions(contact), { initialProps: { contact: 1 } });
        await waitFor(() => expect(result.current.questions).toEqual([first]));
        const oldCancel = result.current.cancel;
        rerender({ contact: 2 });
        expect(result.current.questions).toEqual([]);
        await act(async () => oldCancel(first));
        expect(mocks.invoke.mock.calls.every(([command]) => command === "list_butler_contact_task_questions")).toBe(true);
    });

    it("should reject duplicate submissions while the scoped response is pending", async () => {
        const item = question({}); const submitted = deferred<void>();
        mocks.invoke.mockImplementation((command: string) => command === "list_butler_contact_task_questions" ? Promise.resolve([item]) : submitted.promise);
        const { result } = renderHook(() => useButlerTaskQuestions(1));
        await waitFor(() => expect(result.current.questions).toEqual([item]));
        let first!: Promise<void>;
        act(() => { first = result.current.submit(item, { 格式: "PDF" }); });
        await act(async () => result.current.submit(item, { 格式: "PDF" }));
        expect(mocks.invoke.mock.calls.filter(([command]) => command === "respond_butler_contact_task_question")).toHaveLength(1);
        await act(async () => { submitted.resolve(); await first; });
    });

    it("should render answered questions as persistent read-only history", () => {
        const submit = vi.fn(); const cancel = vi.fn();
        const items = taskQuestionItems([question({ request_id: null, history_key: "request:1", answers: { 格式: "PDF" } })], "写作", submit, cancel);
        render(<>{items.map(item => <div key={item.key}>{item.content}</div>)}</>);
        expect(screen.getByText(/周报.*已回答/)).toBeVisible();
        expect(screen.getByText(/已答.*PDF/)).toBeVisible();
        expect(screen.queryByRole("button", { name: "提交" })).toBeNull();
    });
    it("should show an interrupted handoff as unconfirmed without retry controls", () => {
        const items = taskQuestionItems([question({ request_id: null, history_key: "call:1", answers: { 格式: "PDF" }, delivery_status: "pending" })], "写作", vi.fn(), vi.fn());
        render(<>{items.map(item => <div key={item.key}>{item.content}</div>)}</>);
        expect(screen.getByText(/送达状态未确认.*不要重复提交/)).toBeVisible();
        expect(screen.queryByRole("button", { name: "提交" })).toBeNull();
    });

});
