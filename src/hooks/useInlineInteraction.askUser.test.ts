import { describe, expect, it } from "vitest";
import { askUserQuestionStateForTest } from "./useInlineInteraction";
import type { MCPToolCall } from "@/data/MCPToolCall";

const { getLatestAskUserQuestionState, resolveAskUserQuestionView, buildAskUserQuestionSignature } = askUserQuestionStateForTest;

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


function makeRequest(
    id: string,
    question: string,
    header = "Q"
): AskUserQuestionRequestLike {
    return {
        request_id: id,
        conversation_id: 1,
        questions: [
            {
                header,
                question,
                multiSelect: false,
                options: [
                    { label: "A", description: "" },
                    { label: "B", description: "" },
                ],
            },
        ],
    };
}

// Local structural type so the test file does not need to import the card module.
type AskUserQuestionRequestLike = {
    request_id: string;
    conversation_id?: number;
    questions: {
        header: string;
        question: string;
        multiSelect: boolean;
        options: { label: string; description: string }[];
    }[];
};

describe("resolveAskUserQuestionView — clarified sticky-summary path", () => {
    it("does not keep centered Q1 answers when Q2 is pending (waiting for Run)", () => {
        const q1 = makeRequest("live-q1", "E2E第一题：你最喜欢的颜色是？", "颜色");
        const q2 = makeRequest("persisted-ask-2", "E2E第二题：选择部署环境？", "环境");
        const view = resolveAskUserQuestionView({
            pendingRequest: null,
            isDialogOpen: false,
            submittedSummary: {
                request: q1 as any,
                answers: { "E2E第一题：你最喜欢的颜色是？": "红色" },
            },
            persistedState: {
                callId: 2,
                messageId: 10,
                request: q2 as any,
                status: "pending",
                answers: null,
            },
        });
        expect(view.isOpen).toBe(false);
        expect(view.mode).toBe("questionnaire");
        expect(view.completedAnswers).toBeNull();
        expect(view.request).toBeNull();
    });

    it("shows Q2 questionnaire when Q2 is executing and summary is for Q1", () => {
        const q1 = makeRequest("live-q1", "E2E第一题：你最喜欢的颜色是？", "颜色");
        const q2 = makeRequest("persisted-ask-2", "E2E第二题：选择部署环境？", "环境");
        const view = resolveAskUserQuestionView({
            pendingRequest: null,
            isDialogOpen: false,
            submittedSummary: {
                request: q1 as any,
                answers: { "E2E第一题：你最喜欢的颜色是？": "红色" },
            },
            persistedState: {
                callId: 2,
                messageId: 10,
                request: q2 as any,
                status: "executing",
                answers: null,
            },
        });
        expect(view.isOpen).toBe(true);
        expect(view.mode).toBe("questionnaire");
        expect(view.request?.questions[0]?.question).toContain("第二题");
        expect(view.completedAnswers).toBeNull();
    });

    it("keeps same-call summary while Q1 is still executing after submit", () => {
        const q1 = makeRequest("live-q1", "E2E第一题：你最喜欢的颜色是？", "颜色");
        const view = resolveAskUserQuestionView({
            pendingRequest: null,
            isDialogOpen: false,
            submittedSummary: {
                request: q1 as any,
                answers: { "E2E第一题：你最喜欢的颜色是？": "红色" },
            },
            persistedState: {
                callId: 1,
                messageId: 10,
                request: {
                    ...q1,
                    request_id: "persisted-ask-1",
                } as any,
                status: "executing",
                answers: null,
            },
        });
        // Signatures match → summaryMatchesPersisted; keep ephemeral summary.
        expect(buildAskUserQuestionSignature(q1 as any)).toBe(
            buildAskUserQuestionSignature({
                ...q1,
                request_id: "persisted-ask-1",
            } as any)
        );
        expect(view.isOpen).toBe(true);
        expect(view.mode).toBe("summary");
        expect(view.completedAnswers?.["E2E第一题：你最喜欢的颜色是？"]).toBe("红色");
    });

    it("live queued Q2 prompt wins over Q1 summary", () => {
        const q1 = makeRequest("live-q1", "E2E第一题：你最喜欢的颜色是？", "颜色");
        const q2 = makeRequest("live-q2", "E2E第二题：选择部署环境？", "环境");
        const view = resolveAskUserQuestionView({
            pendingRequest: q2 as any,
            isDialogOpen: true,
            submittedSummary: {
                request: q1 as any,
                answers: { "E2E第一题：你最喜欢的颜色是？": "红色" },
            },
            persistedState: null,
        });
        expect(view.isOpen).toBe(true);
        expect(view.mode).toBe("questionnaire");
        expect(view.request?.questions[0]?.question).toContain("第二题");
    });
});
