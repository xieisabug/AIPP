import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { clearAllMockHandlers, invoke, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import AssistantAvatarSettings from "./AssistantAvatarSettings";

beforeEach(() => {
    clearAllMockHandlers();
    invoke.mockClear();
    mockInvokeHandler("get_all_feature_config", () => [
        { feature_code: "butler_appearance", key: "butler_appearance_9", value: "other-avatar" },
    ]);
    mockInvokeHandler("save_feature_config", () => null);
});

describe("AssistantAvatarSettings", () => {
    it("should preserve other avatars and avoid changing the name when saving an avatar", async () => {
        render(<AssistantAvatarSettings id={5} name="Personal assistant" />);
        await waitFor(() => expect(screen.getByRole("button", { name: "编辑头像" })).toBeEnabled());
        fireEvent.click(screen.getByRole("button", { name: "编辑头像" }));
        expect(screen.queryByLabelText("助手名称")).not.toBeInTheDocument();
        fireEvent.click(screen.getByRole("button", { name: "脸部：圆框眼镜" }));
        fireEvent.click(screen.getByRole("button", { name: /^保存$/ }));
        await waitFor(() => expect(invoke).toHaveBeenCalledWith("save_feature_config", expect.objectContaining({
            featureCode: "butler_appearance",
            config: { butler_appearance_9: "other-avatar", butler_appearance_5: expect.stringContaining('"eyes":"round-glasses"') },
        })));
        expect(invoke.mock.calls.some(([command]) => command === "save_assistant")).toBe(false);
        await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    });
});
