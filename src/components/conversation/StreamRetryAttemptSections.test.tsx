import { ButlerResultsOnlyContext } from "../butler/ButlerPresentationContext";
import { describe, expect, it, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import type { Message } from "@/data/Conversation";
import { StreamRetryAttemptSections } from "./StreamRetryAttemptSections";

function makeMessage(
    id: number,
    content: string,
    metadata_json?: string,
): Message {
    return {
        id,
        conversation_id: 1,
        message_type: "response",
        content,
        llm_model_id: null,
        created_time: new Date("2026-11-01T00:00:00Z"),
        start_time: new Date("2026-11-01T00:00:00Z"),
        finish_time: new Date("2026-11-01T00:00:01Z"),
        token_count: 0,
        input_token_count: 0,
        output_token_count: 0,
        generation_group_id: null,
        parent_group_id: null,
        regenerate: null,
        metadata_json,
    };
}

describe("StreamRetryAttemptSections", () => {
    it("renders flat content when there is no retry metadata", () => {
        render(
            <StreamRetryAttemptSections
                messages={[makeMessage(1, "hello")]}
                renderMessages={(messages) => (
                    <div>{messages.map((message) => message.content).join("|")}</div>
                )}
            />,
        );
        expect(screen.getByText("hello")).toBeInTheDocument();
        expect(screen.queryByText(/尝试/)).not.toBeInTheDocument();
    });

    it("should omit retry diagnostics when the Butler transcript is compact", () => {
        const messages = [makeMessage(1, "最终答复", JSON.stringify({
            stream_retry: {session_id:"s1",attempt:2,status:"completed",error:"raw transport failure"},
        }))];
        render(<ButlerResultsOnlyContext.Provider value={true}>
            <StreamRetryAttemptSections messages={messages} renderMessages={items => <>{items[0].content}</>}/>
        </ButlerResultsOnlyContext.Provider>);
        expect(screen.getByText("最终答复")).toBeVisible();
        expect(screen.queryByText(/raw transport/)).not.toBeInTheDocument();
        expect(screen.queryByText(/尝试/)).not.toBeInTheDocument();
    });

    it("groups interrupted and completed attempts and collapses older partials", () => {
        const messages = [
            makeMessage(
                1,
                "partial-one",
                JSON.stringify({
                    stream_retry: {
                        session_id: "s1",
                        attempt: 1,
                        status: "interrupted",
                        error: "stream dropped",
                    },
                }),
            ),
            makeMessage(
                2,
                "final-two",
                JSON.stringify({
                    stream_retry: {
                        session_id: "s1",
                        attempt: 2,
                        status: "completed",
                    },
                }),
            ),
        ];

        render(
            <StreamRetryAttemptSections
                messages={messages}
                renderMessages={(chunk) => (
                    <div data-testid={`chunk-${chunk[0].id}`}>
                        {chunk.map((message) => message.content).join("|")}
                    </div>
                )}
            />,
        );

        expect(screen.getByText("尝试 1")).toBeInTheDocument();
        expect(screen.getByText("已中断")).toBeInTheDocument();
        expect(screen.getByText("尝试 2")).toBeInTheDocument();
        expect(screen.getByText("完成")).toBeInTheDocument();
        // Latest attempt content visible by default
        expect(screen.getByTestId("chunk-2")).toHaveTextContent("final-two");
        // Expand interrupted attempt
        fireEvent.click(screen.getByText("尝试 1"));
        expect(screen.getByTestId("chunk-1")).toHaveTextContent("partial-one");

        const copyButtons = screen.getAllByRole("button", { name: "复制此尝试内容" });
        expect(copyButtons.length).toBeGreaterThan(0);
        for (const button of copyButtons) {
            expect(button).toHaveClass("min-h-11", "min-w-11", "h-11", "w-11");
        }
    });

    it("shows the full interruption diagnostic on tap and copies it on its own", async () => {
        const writeText = vi.fn().mockResolvedValue(undefined);
        Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
        const longError = "upstream closed connection: " + "x".repeat(300) + " [request_id=abc]";
        const messages = [
            makeMessage(1, "partial-one", JSON.stringify({
                stream_retry: { session_id: "s1", attempt: 1, status: "interrupted", error: longError },
            })),
            makeMessage(2, "final-two", JSON.stringify({
                stream_retry: { session_id: "s1", attempt: 2, status: "completed" },
            })),
        ];
        render(
            <StreamRetryAttemptSections
                messages={messages}
                renderMessages={(chunk) => <div>{chunk.map((message) => message.content).join("|")}</div>}
            />,
        );

        // 标题摘要不再依赖 title 悬停
        expect(document.querySelector(`[title]`)).toBeNull();

        fireEvent.click(screen.getByText("尝试 1"));
        const diagnostic = document.querySelector("[data-stream-retry-error]");
        expect(diagnostic).not.toBeNull();
        expect(diagnostic).toHaveTextContent("[request_id=abc]");

        fireEvent.click(screen.getByRole("button", { name: "复制中断原因" }));
        expect(writeText).toHaveBeenCalledWith(longError);
    });
});
