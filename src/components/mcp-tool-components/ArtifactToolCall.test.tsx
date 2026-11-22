import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";

const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => mockInvoke(...args) }));
const toastError = vi.fn();
vi.mock("sonner", () => ({ toast: { error: (...args: unknown[]) => toastError(...args) } }));

import ArtifactToolCall, { parseShowArtifactError, parseShowArtifactResult } from "./ArtifactToolCall";

const RESULT = JSON.stringify([
    {
        type: "json",
        json: {
            artifact_key: "tools/ledger",
            title: "记账本",
            language: "tsx",
            preview_type: "tsx",
            entry_file: "App.tsx",
            db_id: "ledger",
            assistant_id: 3,
        },
    },
]);

function call(status: string, extra: Record<string, unknown> = {}) {
    return { call_id: 1, conversation_id: 9, status, ...extra } as never;
}

describe("ArtifactToolCall", () => {
    beforeEach(() => {
        mockInvoke.mockReset();
        toastError.mockReset();
    });

    it("should parse stored results and tool errors", () => {
        expect(parseShowArtifactResult(RESULT)?.artifact_key).toBe("tools/ledger");
        expect(parseShowArtifactResult("not json")).toBeNull();
        expect(
            parseShowArtifactError('工具执行错误: [{"type":"text","text":"Artifact entry file does not exist: C:/x/App.tsx"}]'),
        ).toBe("Artifact entry file does not exist: C:/x/App.tsx");
        expect(parseShowArtifactError(undefined)).toBe("未说明原因");
    });

    it("should show progress until the artifact is published", () => {
        render(
            <ArtifactToolCall
                toolName="show_artifact"
                status="executing"
                currentToolCall={call("executing", { parameters: JSON.stringify({ title: "记账本" }) })}
            />,
        );
        expect(screen.getByTestId("artifact-card")).toHaveAttribute("data-state", "running");
        expect(screen.getByText(/记账本/)).toBeInTheDocument();
    });

    it("should explain why publishing failed", () => {
        render(
            <ArtifactToolCall
                toolName="show_artifact"
                status="failed"
                currentToolCall={call("failed", { error: '工具执行错误: [{"type":"text","text":"Unsupported artifact language \'py\'"}]' })}
            />,
        );
        expect(screen.getByTestId("artifact-card")).toHaveAttribute("data-state", "failed");
        expect(screen.getByText("Unsupported artifact language 'py'")).toBeInTheDocument();
    });

    it("should open the latest version with its database and assistant bindings", async () => {
        mockInvoke.mockImplementation(async (command: string) => {
            if (command === "list_conversation_artifacts") {
                return [
                    {
                        artifact_key: "tools/ledger",
                        title: "记账本",
                        language: "tsx",
                        preview_type: "tsx",
                        entry_file: "App.tsx",
                        code: "export default function App() { return <div /> }",
                        updated_at: "",
                        db_id: "ledger",
                        assistant_id: 3,
                    },
                ];
            }
            return "ok";
        });
        render(<ArtifactToolCall toolName="show_artifact" status="success" currentToolCall={call("success", { result: RESULT })} />);
        expect(screen.getByText("记账本")).toBeInTheDocument();
        expect(screen.getByText("React · Artifact")).toBeInTheDocument();

        fireEvent.click(screen.getByRole("button", { name: "打开" }));
        await waitFor(() =>
            expect(mockInvoke).toHaveBeenCalledWith("run_artifacts", {
                lang: "tsx",
                inputStr: "export default function App() { return <div /> }",
                conversationId: 9,
                dbId: "ledger",
                assistantId: 3,
                artifactKey: "tools/ledger",
                entryFile: "App.tsx",
            }),
        );
        expect(mockInvoke).toHaveBeenCalledWith("list_conversation_artifacts", { conversationId: 9 });
    });

    it("should tell the user when the artifact no longer exists", async () => {
        mockInvoke.mockResolvedValue([]);
        render(<ArtifactToolCall toolName="show_artifact" status="success" currentToolCall={call("success", { result: RESULT })} />);
        fireEvent.click(screen.getByRole("button", { name: "打开" }));
        await waitFor(() => expect(toastError).toHaveBeenCalledWith("找不到 Artifact「记账本」，可能已被删除"));
        expect(mockInvoke).not.toHaveBeenCalledWith("run_artifacts", expect.anything());
    });
});
