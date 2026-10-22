import { describe, expect, it } from "vitest";
import { askUserQuestionStateForTest } from "./useInlineInteraction";
import type { MCPToolCall } from "@/data/MCPToolCall";

const { getLatestAskUserQuestionState } = askUserQuestionStateForTest;

function makeAskCall(
    id: number,
    status: MCPToolCall["status"],
    question: string,
    result?: string
): MCPToolCall {
    return {
        id,
        conversation_id: 1,
        message_id: 10,
        server_id: 1,
        server_name: "UI交互工具",
        tool_name: "ask_user_question",
        parameters: JSON.stringify({
            questions: [
                {
                    header: `Q${id}`,
                    question,
                    multiSelect: false,
                    options: [
                        { label: "A", description: "" },
                        { label: "B", description: "" },
                    ],
                },
            ],
        }),
        status,
        result,
        created_time: "",
    } as MCPToolCall;
}

describe("getLatestAskUserQuestionState", () => {
    it("prefers a newer pending/executing ask over an older success", () => {
        const calls = [
            makeAskCall(
                1,
                "success",
                "第一题：喜欢什么颜色？",
                JSON.stringify({ answers: { "第一题：喜欢什么颜色？": "A" } })
            ),
            makeAskCall(2, "pending", "第二题：选择部署环境？"),
        ];
        const state = getLatestAskUserQuestionState(calls, 1);
        expect(state?.callId).toBe(2);
        expect(state?.status).toBe("pending");
        expect(state?.request.questions[0]?.question).toContain("第二题");
        expect(state?.answers).toBeNull();
    });

    it("prefers executing over a higher-id success that should not win while in flight", () => {
        const calls = [
            makeAskCall(3, "executing", "进行中的问题？"),
            makeAskCall(
                2,
                "success",
                "已完成的问题？",
                JSON.stringify({ answers: { "已完成的问题？": "B" } })
            ),
        ];
        const state = getLatestAskUserQuestionState(calls, 1);
        expect(state?.callId).toBe(3);
        expect(state?.status).toBe("executing");
    });

    it("prefers an older executing ask over a newer pending ask", () => {
        const calls = [
            makeAskCall(1, "executing", "第一题：仍在作答"),
            makeAskCall(2, "pending", "第二题：尚未开始"),
        ];
        const state = getLatestAskUserQuestionState(calls, 1);
        expect(state?.callId).toBe(1);
        expect(state?.status).toBe("executing");
        expect(state?.request.questions[0]?.question).toContain("第一题");
    });

    it("falls back to latest success when nothing is in flight", () => {
        const calls = [
            makeAskCall(
                1,
                "success",
                "旧问题",
                JSON.stringify({ answers: { 旧问题: "A" } })
            ),
            makeAskCall(
                2,
                "success",
                "新问题",
                JSON.stringify({ answers: { 新问题: "B" } })
            ),
        ];
        const state = getLatestAskUserQuestionState(calls, 1);
        expect(state?.callId).toBe(2);
        expect(state?.status).toBe("success");
        expect(state?.answers?.["新问题"]).toBe("B");
    });
});
