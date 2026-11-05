import { fireEvent, render, screen, waitFor, cleanup } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import ButlerProfileDialog from "./ButlerProfileDialog";

describe("Butler profile editor", () => {
    afterEach(cleanup);

    it("should save trimmed name and independent colors when confirming the preview", async () => {
        const onSave = vi.fn().mockResolvedValue(undefined);
        const onClose = vi.fn();
        render(<ButlerProfileDialog name="总管家" onSave={onSave} onClose={onClose} />);
        fireEvent.change(screen.getByLabelText("助手名称"), { target: { value: " 小管家 " } });
        fireEvent.click(screen.getByRole("button", { name: "左侧 · 亮色：薄荷绿" }));
        fireEvent.click(screen.getByRole("button", { name: "右侧 · 深色：海军蓝" }));
        expect(onSave).not.toHaveBeenCalled();
        fireEvent.click(screen.getByRole("button", { name: "保存" }));
        await waitFor(() => expect(onClose).toHaveBeenCalledOnce());
        expect(onSave).toHaveBeenCalledWith("小管家", { shape: "aipp", left: "#CEF3DF", right: "#193B68" });
    });

    it("should discard edits when cancelled and reject blank names", () => {
        const onSave = vi.fn();
        const onClose = vi.fn();
        render(<ButlerProfileDialog name="总管家" onSave={onSave} onClose={onClose} />);
        fireEvent.change(screen.getByLabelText("助手名称"), { target: { value: "  " } });
        expect(screen.getByRole("button", { name: "保存" })).toBeDisabled();
        fireEvent.click(screen.getByRole("button", { name: "取消" }));
        expect(onSave).not.toHaveBeenCalled();
        expect(onClose).toHaveBeenCalledOnce();
    });

    it("should preserve draft and show the underlying error when saving fails", async () => {
        const onClose = vi.fn();
        render(<ButlerProfileDialog name="总管家" onSave={vi.fn().mockRejectedValue(new Error("配置写入失败：database is locked"))} onClose={onClose} />);
        fireEvent.click(screen.getByRole("button", { name: "保存" }));
        expect(await screen.findByRole("alert")).toHaveTextContent("database is locked");
        expect(screen.getByLabelText("助手名称")).toHaveValue("总管家");
        expect(onClose).not.toHaveBeenCalled();
    });
});
