import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import MobileCollections from "./MobileCollections";

const preview = vi.hoisted(() => vi.fn());
vi.mock("./navigation", () => ({ previewMobile: preview }));
vi.mock("@/hooks/use-platform", () => ({ usePlatform: () => "android" }));
const item = { id: 7, name: "Sample", artifact_type: "html", code: "<p>hello</p>" };

describe("MobileCollections", () => {
    beforeEach(() => { clearAllMockHandlers(); preview.mockClear(); });
    it("loads the selected detail and opens preview", async () => {
        mockInvokeHandler("get_artifacts_collection", () => [item]);
        mockInvokeHandler("get_artifact_by_id", () => item);
        render(<MobileCollections />);
        fireEvent.click(await screen.findByRole("button", { name: "Sample" }));
        await vi.waitFor(() => expect(preview).toHaveBeenCalledWith({ title: "Sample", language: "html", code: "<p>hello</p>", collectionId: 7 }));
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
        expect(screen.getAllByRole("button", { name: "新建收藏" }).length).toBeGreaterThan(0);
    });
    it("creates a collection from the mobile entry", async () => {
        mockInvokeHandler("get_artifacts_collection", () => []);
        const saved: unknown[] = [];
        mockInvokeHandler("save_artifact_to_collection", (args: any) => {
            saved.push(args.request);
            return 42;
        });
        render(<MobileCollections />);
        fireEvent.click((await screen.findAllByRole("button", { name: "新建收藏" }))[0]);
        fireEvent.change(screen.getByLabelText("收藏名称"), { target: { value: "笔记" } });
        fireEvent.change(screen.getByLabelText("收藏内容"), { target: { value: "# hello" } });
        fireEvent.click(screen.getByRole("button", { name: "保存" }));
        await vi.waitFor(() => expect(saved[0]).toMatchObject({
            name: "笔记",
            artifact_type: "markdown",
            code: "# hello",
        }));
    });
    it("renames a collection item", async () => {
        mockInvokeHandler("get_artifacts_collection", () => [item]);
        const updates: unknown[] = [];
        mockInvokeHandler("update_artifact_collection", (args: any) => { updates.push(args.request); return null; });
        render(<MobileCollections />);
        await screen.findByText("Sample");
        fireEvent.click(screen.getByRole("button", { name: "重命名" }));
        fireEvent.change(screen.getByLabelText("新名称"), { target: { value: "  Renamed " } });
        fireEvent.click(screen.getByRole("button", { name: "保存名称" }));
        await vi.waitFor(() => expect(updates[0]).toEqual({ id: 7, name: "Renamed" }));
    });
});
