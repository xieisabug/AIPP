import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import EditAssistantDialog from "./EditAssistantDialog";
import type { AssistantDetail } from "@/data/Assistant";
import { defaultAppearance } from "@/components/butler/butlerAppearance";

const mocks = vi.hoisted(() => ({ load: vi.fn(), save: vi.fn() }));
vi.mock("@/hooks/feature/useFeatureConfig", () => ({
    useFeatureConfig: () => ({ loadFeatureConfig: mocks.load, saveFeatureConfig: mocks.save }),
}));
const assistant = { assistant: { id: 7, name: "助手", description: "原描述" } } as AssistantDetail;
const existing = new Map([["butler_appearance", new Map([
    ["butler_appearance_7", JSON.stringify({ ...defaultAppearance, shape: "pebble" })],
    ["butler_appearance_chief", "chief-profile"],
])]]);

describe("EditAssistantDialog shared profile", () => {
    beforeEach(() => { vi.clearAllMocks(); mocks.load.mockResolvedValue(existing); mocks.save.mockResolvedValue(undefined); });
    it("should load the Butler appearance and preserve other profiles when saving assistant edits", async () => {
        const onSave = vi.fn().mockResolvedValue(undefined);
        const onClose = vi.fn();
        render(<EditAssistantDialog isOpen currentAssistant={assistant} onSave={onSave} onClose={onClose} onAssistantUpdated={vi.fn()} />);
        expect(await screen.findByLabelText("助手名称")).toHaveValue("助手");
        expect(screen.getByRole("button", { name: "圆团" })).toHaveAttribute("aria-pressed", "true");
        fireEvent.change(screen.getByLabelText("描述（选填）"), { target: { value: "新描述" } });
        fireEvent.click(screen.getByRole("button", { name: "保存" }));
        await waitFor(() => expect(onClose).toHaveBeenCalledOnce());
        expect(onSave).toHaveBeenCalledWith({ assistant: { ...assistant.assistant, description: "新描述" } });
        expect(mocks.save).toHaveBeenCalledWith("butler_appearance", {
            butler_appearance_7: JSON.stringify({ ...defaultAppearance, shape: "pebble" }),
            butler_appearance_chief: "chief-profile",
        });
    });
    it("should retain the draft and explain partial failure when appearance persistence fails", async () => {
        mocks.save.mockRejectedValue(new Error("database locked"));
        const onClose = vi.fn();
        render(<EditAssistantDialog isOpen currentAssistant={assistant} onSave={vi.fn().mockResolvedValue(undefined)} onClose={onClose} onAssistantUpdated={vi.fn()} />);
        await screen.findByLabelText("助手名称");
        fireEvent.click(screen.getByRole("button", { name: "保存" }));
        expect(await screen.findByRole("alert")).toHaveTextContent("名字与描述已保存，但形象保存失败（助手 7）：database locked");
        expect(onClose).not.toHaveBeenCalled();
    });
    it("should stop editing when the existing appearance cannot be read", async () => {
        mocks.load.mockRejectedValue(new Error("offline"));
        render(<EditAssistantDialog isOpen currentAssistant={assistant} onSave={vi.fn()} onClose={vi.fn()} onAssistantUpdated={vi.fn()} />);
        expect(await screen.findByRole("alert")).toHaveTextContent("读取助手 7 形象失败");
        expect(screen.queryByRole("button", { name: "保存" })).toBeNull();
    });
});
