import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import MobileCollections from "./MobileCollections";

const preview = vi.hoisted(() => vi.fn());
vi.mock("./navigation", () => ({ previewMobile: preview }));
const item = { id: 7, name: "Sample", artifact_type: "html", code: "<p>hello</p>" };

describe("MobileCollections", () => {
    beforeEach(() => { clearAllMockHandlers(); preview.mockClear(); });
    it("loads the selected detail and opens preview", async () => {
        mockInvokeHandler("get_artifacts_collection", () => [item]);
        mockInvokeHandler("get_artifact_by_id", () => item);
        render(<MobileCollections />);
        fireEvent.click(await screen.findByRole("button", { name: "Sample" }));
        await vi.waitFor(() => expect(preview).toHaveBeenCalledWith({ title: "Sample", language: "html", code: "<p>hello</p>" }));
    });
    it("keeps the item and reports a failed delete", async () => {
        mockInvokeHandler("get_artifacts_collection", () => [item]);
        mockInvokeHandler("delete_artifact_collection", () => { throw new Error("denied"); });
        render(<MobileCollections />);
        await screen.findByText("Sample");
        fireEvent.click(screen.getByRole("button", { name: "删除" }));
        fireEvent.click(screen.getByRole("button", { name: "确认删除" }));
        expect(await screen.findByRole("alert")).toHaveTextContent("denied");
        expect(screen.getByText("Sample")).toBeTruthy();
        expect(screen.getByRole("button", { name: "确认删除" })).toBeTruthy();
    });
    it("shows an empty state after loading", async () => {
        mockInvokeHandler("get_artifacts_collection", () => []);
        render(<MobileCollections />);
        expect(await screen.findByText("暂无收藏")).toBeTruthy();
    });
});
