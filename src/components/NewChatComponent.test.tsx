import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import NewChatComponent from "./NewChatComponent";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
    invoke: (...args: unknown[]) => invokeMock(...args),
}));

vi.mock("../hooks/use-mobile", () => ({
    useIsMobile: () => false,
}));

vi.mock("./AskWindowPrepare", () => ({
    default: () => null,
}));

describe("NewChatComponent Agent Plan mode", () => {
    beforeEach(() => {
        invokeMock.mockReset();
    });

    it.each([
        ["codex_app_server", "Codex"],
        ["claude_sdk", "Claude Code"],
    ])("shows the Plan control for %s", async (agentKind) => {
        invokeMock.mockImplementation((command: string) => {
            if (command === "get_assistant") {
                return Promise.resolve({ assistant: { assistant_type: 4 }, model: [], model_configs: [] });
            }
            if (command === "get_agent_runtime_info") {
                return Promise.resolve({ agent_kind: agentKind });
            }
            if (command === "get_agent_model_options") {
                return Promise.resolve([]);
            }
            if (command === "get_codex_agent_defaults") {
                return Promise.resolve(null);
            }
            return Promise.reject(new Error(`unexpected command: ${command}`));
        });
        const onAgentModeChange = vi.fn();

        render(
            <NewChatComponent
                selectedText=""
                selectedAssistant={1}
                setSelectedAssistant={vi.fn()}
                assistants={[{ id: 1, name: "Agent", assistant_type: 4 }]}
                selectedModel=""
                selectedEffort=""
                selectedApprovalPolicy=""
                selectedSandbox=""
                selectedMode="default"
                onAgentModeChange={onAgentModeChange}
                onAgentConfigChange={vi.fn()}
            />,
        );

        const button = await screen.findByRole("button", { name: "Plan" });
        fireEvent.click(button);
        expect(onAgentModeChange).toHaveBeenLastCalledWith("plan");
        await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("get_agent_runtime_info", { assistantId: 1 }));
    });
});
