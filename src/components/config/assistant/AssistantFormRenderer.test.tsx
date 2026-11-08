import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import { AssistantFormRenderer } from "./AssistantFormRenderer";

vi.mock("@/hooks/use-platform", () => ({ useIsMobilePlatform: () => true }));
vi.mock("@/components/ConfigForm", () => ({ default: ({ advancedFieldKeys, extensionFieldKeys }: { advancedFieldKeys: string[]; extensionFieldKeys: string[] }) =>
    <div data-testid="form" data-advanced={advancedFieldKeys.join(",")} data-extension={extensionFieldKeys.join(",")}>editable form</div> }));
const props = { currentAssistant: { assistant: { id: 5, name: "Test assistant", description: "saved", assistant_type: 2 } }, formConfig: [], form: {}, assistantConfigApi: {}, onSave: vi.fn() } as unknown as React.ComponentProps<typeof AssistantFormRenderer>;

describe("AssistantFormRenderer mobile capability", () => {
    beforeEach(() => clearAllMockHandlers());
    it.each([0, 4])("should reserve extensions for plugins when rendering assistant type %s", async (assistantType) => {
        mockInvokeHandler("get_assistant_execution_capability", () => ({ available: true, issues: [] }));
        render(<AssistantFormRenderer {...props}
            currentAssistant={{ ...props.currentAssistant!, assistant: { ...props.currentAssistant!.assistant, assistant_type: assistantType } }}
            formConfig={[
                { key: "mcp_config", config: { type: "custom", label: "MCP" } },
                { key: "skills_config", config: { type: "custom", label: "Skills" } },
                { key: "plugin::1::enabled", section: "extension", config: { type: "checkbox", label: "Plugin" } },
            ]} />);
        const form = await screen.findByTestId("form");
        expect(form.getAttribute("data-advanced")).toContain("mcp_config");
        expect(form.getAttribute("data-advanced")).toContain("skills_config");
        expect(form.getAttribute("data-extension")).toBe("plugin::1::enabled");
    });
    it("keeps an incompatible assistant read-only", async () => {
        mockInvokeHandler("get_assistant_execution_capability", () => ({ available: false, issues: [{ capability: "local_cli_agent", reason: "desktop only" }] }));
        render(<AssistantFormRenderer {...props} />);
        expect(await screen.findByText("desktop only")).toBeTruthy();
        expect(screen.getByText(/移动端只读/)).toBeTruthy();
        expect(screen.queryByText("editable form")).toBeNull();
    });
    it("shows the failure and keeps the form read-only if capability lookup fails", async () => {
        mockInvokeHandler("get_assistant_execution_capability", () => { throw new Error("capability offline"); });
        render(<AssistantFormRenderer {...props} />);
        expect(await screen.findByText(/能力校验失败.*capability offline/)).toBeTruthy();
        expect(screen.getByText(/移动端只读/)).toBeTruthy();
        expect(screen.queryByText("editable form")).toBeNull();
    });
});
