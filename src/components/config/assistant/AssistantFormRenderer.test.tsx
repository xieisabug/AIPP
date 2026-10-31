import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import { AssistantFormRenderer } from "./AssistantFormRenderer";

vi.mock("@/hooks/use-platform", () => ({ useIsMobilePlatform: () => true }));
vi.mock("@/components/ConfigForm", () => ({ default: () => <div>editable form</div> }));
const props = { currentAssistant: { assistant: { id: 5, name: "Test assistant", description: "saved", assistant_type: 2 } }, formConfig: [], form: {}, assistantConfigApi: {}, onSave: vi.fn() } as unknown as React.ComponentProps<typeof AssistantFormRenderer>;

describe("AssistantFormRenderer mobile capability", () => {
    beforeEach(() => clearAllMockHandlers());
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
