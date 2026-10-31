import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { mobileFileAction } from "@/services/mobileFileService";
import MobileSkillsManager from "./MobileSkillsManager";

const mocks = vi.hoisted(() => ({ enable: vi.fn(), success: vi.fn(), error: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@/services/mobileFileService", async importOriginal => ({ ...await importOriginal<typeof import("@/services/mobileFileService")>(), mobileFileAction: vi.fn() }));
vi.mock("@/hooks/useSkillsMcpValidation", () => ({ useSkillsMcpValidation: () => ({ enableOperationMcpAndSkill: mocks.enable }) }));
vi.mock("sonner", () => ({ toast: { success: mocks.success, error: mocks.error } }));

describe("mobile skills", () => {
    beforeEach(() => {
        vi.clearAllMocks();
        vi.mocked(invoke).mockImplementation(async (command) => {
            if (command === "scan_skills") return [{ identifier: "demo", display_name: "Demo" }];
            if (command === "get_assistant_skills") return [];
            if (command === "get_skill_content") return { content: "instructions" };
            if (command === "get_mobile_skill_compatibility") return "本地脚本不可执行";
            if (command === "install_mobile_skill_archive") return ["Demo"];
            throw new Error(`unexpected ${command}`);
        });
    });
    it("should not install on cancellation and should refresh the list", async () => {
        vi.mocked(mobileFileAction).mockResolvedValue({ status: "cancelled" });
        render(<MobileSkillsManager />);
        await screen.findByText("Demo");
        fireEvent.click(screen.getByText("导入 ZIP"));
        await waitFor(() => expect(vi.mocked(invoke).mock.calls.filter(([name]) => name === "scan_skills")).toHaveLength(2));
        expect(invoke).not.toHaveBeenCalledWith("install_mobile_skill_archive", expect.anything());
        expect(mobileFileAction).toHaveBeenCalledWith("pick", { skill: true });
    });
    it("should install the selected archive and refresh", async () => {
        vi.mocked(mobileFileAction).mockResolvedValue({ status: "selected", files: [{ path: "/private/demo.zip", name: "demo.zip", mime: "application/zip", size: 5 }] });
        render(<MobileSkillsManager />);
        await screen.findByText("Demo");
        fireEvent.click(screen.getByText("导入 ZIP"));
        await waitFor(() => expect(invoke).toHaveBeenCalledWith("install_mobile_skill_archive", { path: "/private/demo.zip" }));
        await waitFor(() => expect(vi.mocked(invoke).mock.calls.filter(([name]) => name === "scan_skills")).toHaveLength(2));
        expect(mocks.success).toHaveBeenCalledWith(expect.stringContaining("兼容性"));
    });
    it("should show compatibility and bind the selected skill to the assistant", async () => {
        render(<MobileSkillsManager assistantId={42} />);
        fireEvent.click(await screen.findByText("Demo"));
        expect(await screen.findByRole("status")).toHaveTextContent("本地脚本不可执行");
        fireEvent.click(screen.getByText("启用技能加载工具并绑定"));
        await waitFor(() => expect(mocks.enable).toHaveBeenCalledWith(42, "demo", 0));
    });
});
