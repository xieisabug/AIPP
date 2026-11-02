import { describe, expect, it } from "vitest";
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
    });
});
