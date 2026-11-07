import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { taskQuestionItems, type ButlerTaskQuestion } from "./butlerTaskQuestions";

vi.mock("@/components/InlineInteractionCards", () => ({
    AskUserQuestionCard: ({ onSubmit, onCancel }: any) => <div>
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
