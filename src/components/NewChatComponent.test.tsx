import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import NewChatComponent from "./NewChatComponent";

const invokeMock = vi.fn();
const layout = vi.hoisted(() => ({ isNarrow: false, isMobilePlatform: false }));

vi.mock("@tauri-apps/api/core", () => ({
    invoke: (...args: unknown[]) => invokeMock(...args),
}));

vi.mock("../hooks/use-mobile", () => ({
    useIsMobile: () => layout.isNarrow,
}));

vi.mock("../hooks/use-platform", async (importOriginal) => ({
    ...await importOriginal<typeof import("../hooks/use-platform")>(),
    useIsMobilePlatform: () => layout.isMobilePlatform,
}));

describe("NewChatComponent Agent Plan mode", () => {
    beforeEach(() => {
        invokeMock.mockReset();
        layout.isNarrow = false;
        layout.isMobilePlatform = false;
    });

    it.each([
        ["codex_app_server", "Codex"],
        ["claude_sdk", "Claude Code"],
    ])("keeps Plan out of the new-chat config row for %s", async () => {
        invokeMock.mockImplementation((command: string) => {
            if (command === "get_assistant") {
                return Promise.resolve({ assistant: { assistant_type: 4 }, model: [], model_configs: [] });
            }
            if (command === "get_agent_model_options") {
                return Promise.resolve([]);
            }
            if (command === "get_codex_agent_defaults") {
                return Promise.resolve(null);
            }
            return Promise.reject(new Error(`unexpected command: ${command}`));
        });

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
                onAgentConfigChange={vi.fn()}
            />,
        );

        await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("get_assistant", { assistantId: 1 }));
        expect(screen.queryByRole("button", { name: "Plan" })).not.toBeInTheDocument();
    });
});

describe("NewChatComponent home hints", () => {
    it.each([
        { isNarrow: false, isMobilePlatform: false },
        { isNarrow: true, isMobilePlatform: false },
        { isNarrow: false, isMobilePlatform: true },
        { isNarrow: true, isMobilePlatform: true },
    ])("should offer collapsible hints only on mobile when layout is $isNarrow and platform is $isMobilePlatform", async (scenario) => {
        Object.assign(layout, scenario);
        invokeMock.mockImplementation((command: string) => {
            if (command === "get_llm_providers") return Promise.resolve([{ id: 1 }]);
            if (command === "get_assistant") return Promise.resolve({ assistant: { assistant_type: 0 } });
            return Promise.reject(new Error(`unexpected command: ${command}`));
        });
        render(
            <NewChatComponent
                selectedText=""
                selectedAssistant={1}
                setSelectedAssistant={vi.fn()}
                assistants={[{ id: 1, name: "普通助手", assistant_type: 0 }]}
                selectedModel=""
                selectedEffort=""
                selectedApprovalPolicy=""
                selectedSandbox=""
                onAgentConfigChange={vi.fn()}
            />,
        );
        await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("get_llm_providers"));
        if (scenario.isMobilePlatform) {
            const toggle = screen.getByRole("button", { name: /高级提示/ });
            expect(toggle).toHaveAttribute("aria-expanded", "false");
            expect(screen.queryByText("!s")).not.toBeInTheDocument();
            fireEvent.click(toggle);
            expect(toggle).toHaveAttribute("aria-expanded", "true");
            expect(screen.getByText("!s")).toBeVisible();
            fireEvent.click(toggle);
            expect(screen.queryByText("!s")).not.toBeInTheDocument();
        } else {
            expect(screen.queryByRole("button", { name: /高级提示/ })).not.toBeInTheDocument();
            expect(screen.getByText("!s")).toBeVisible();
            expect(screen.getByText("请选择一个对话，或者选择一个助手开始新聊天")).toBeVisible();
        }
    });
});

describe("NewChatComponent setup guide (#155)", () => {
    beforeEach(() => {
        invokeMock.mockReset();
        layout.isNarrow = false;
        layout.isMobilePlatform = true;
    });

    const renderWith = (assistantModel: { provider_id: number; model_code: string } | null) => {
        invokeMock.mockImplementation((command: string) => {
            if (command === "get_llm_providers") return Promise.resolve([{ id: 1, is_enabled: true }]);
            if (command === "get_models_for_select") return Promise.resolve([{ code: "m1", llm_provider_id: 1 }]);
            if (command === "get_assistant") {
                return Promise.resolve({ assistant: { assistant_type: 0 }, model: assistantModel ? [assistantModel] : [], model_configs: [] });
            }
            return Promise.resolve(null);
        });
        render(
            <NewChatComponent
                selectedText=""
                selectedAssistant={1}
                setSelectedAssistant={vi.fn()}
                assistants={[{ id: 1, name: "助手", assistant_type: 0 }]}
                selectedModel=""
                selectedEffort=""
                selectedApprovalPolicy=""
                selectedSandbox=""
                onAgentConfigChange={vi.fn()}
            />,
        );
    };

    it("guides to fix the assistant when its bound model is gone even though a provider exists", async () => {
        renderWith({ provider_id: 9, model_code: "deleted" });
        expect(await screen.findByText(/当前助手的模型不可用/)).toBeInTheDocument();
        expect(screen.getByRole("button", { name: "配置助手" })).toBeInTheDocument();
    });

    it("hides the guide when provider, model and assistant binding are all usable", async () => {
        renderWith({ provider_id: 1, model_code: "m1" });
        await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("get_models_for_select"));
        await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("get_assistant", { assistantId: 1 }));
        expect(document.querySelector('[data-aipp-slot="ask-prepare-setup-guides"]')).toBeNull();
        expect(screen.getByText("输入内容后点击发送，开始与助手交流")).toBeInTheDocument();
    });
});
