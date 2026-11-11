import { cleanup, render, screen, fireEvent, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { StrictMode } from "react";
import { SyncServerSwitchDialog } from "./SyncServerSwitchDialog";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const props = { id: "s", oldUrl: "https://old", serverUrl: "https://new", initialToken: "", onCancel: vi.fn(), onComplete: vi.fn() };
afterEach(() => { cleanup(); vi.resetAllMocks(); });
describe("SyncServerSwitchDialog", () => {
    it("should keep the switch session alive when StrictMode replays effects", async () => {
        vi.mocked(invoke).mockResolvedValue(undefined);
        const view = render(<StrictMode><SyncServerSwitchDialog {...props} /></StrictMode>);
        await Promise.resolve();
        expect(invoke).not.toHaveBeenCalled();
        view.unmount();
        await waitFor(() => expect(invoke).toHaveBeenCalledWith("cancel_sync_server_switch", { id: "s" }));
    });
    it("should require a choice and preserve settings when cancelled", async () => {
        vi.mocked(invoke).mockResolvedValue(undefined);
        render(<SyncServerSwitchDialog {...props} />);
        expect(screen.getByRole("button", { name: "下一步" })).toBeDisabled();
        fireEvent.click(screen.getByRole("button", { name: "取消" }));
        await waitFor(() => expect(props.onCancel).toHaveBeenCalled());
        expect(invoke).toHaveBeenCalledWith("cancel_sync_server_switch", { id: "s" });
        expect(invoke).not.toHaveBeenCalledWith("commit_sync_server_switch", expect.anything());
    });
    it("should inspect then confirm migration without changing the target early", async () => {
        vi.mocked(invoke).mockResolvedValue({ mode: "migration", message: "抽样检查一致", key_known: true, has_remote_key: false });
        render(<SyncServerSwitchDialog {...props} />);
        fireEvent.click(screen.getByRole("radio", { name: /不知道/ }));
        fireEvent.click(screen.getByRole("button", { name: "下一步" }));
        await screen.findByText("抽样检查一致");
        expect(invoke).toHaveBeenCalledWith("inspect_sync_server_switch", { request: { id:"s",server_url:"https://new",token:"",choice:"auto" } });
        expect(props.onComplete).not.toHaveBeenCalled();
        fireEvent.click(screen.getByRole("button", { name: "保存并同步" }));
        await waitFor(() => expect(props.onComplete).toHaveBeenCalled());
        expect(invoke).toHaveBeenCalledWith("commit_sync_server_switch", { id:"s",passphrase:null });
    });
    it("should prevent commit when samples do not match", async () => {
        vi.mocked(invoke).mockResolvedValue({ mode: null, message: "历史位置不一致", key_known: true });
        render(<SyncServerSwitchDialog {...props} />);
        fireEvent.click(screen.getByRole("radio", { name: /不知道/ }));
        fireEvent.click(screen.getByRole("button", { name: "下一步" }));
        await screen.findByText("历史位置不一致");
        expect(screen.queryByRole("button", { name: "保存并同步" })).toBeNull();
    });
    it("should keep the dialog open when the target passphrase is wrong", async () => {
        vi.mocked(invoke).mockResolvedValueOnce({ mode:"new", message:"重新同步", key_known:true,has_remote_key:true });
        render(<SyncServerSwitchDialog {...props} />);
        fireEvent.click(screen.getByRole("radio", { name: /新同步服务器/ }));
        fireEvent.click(screen.getByRole("button", { name: "下一步" }));
        await screen.findByText("重新同步");
        fireEvent.click(screen.getByRole("radio", { name: "输入目标服务器的口令" }));
        fireEvent.change(screen.getByLabelText("目标服务器加密口令"), { target:{value:"wrongpass"} });
        vi.mocked(invoke).mockRejectedValueOnce("口令不匹配");
        fireEvent.click(screen.getByRole("button", { name: "保存并同步" }));
        expect(await screen.findByRole("alert")).toHaveTextContent("口令不匹配");
        expect(props.onComplete).not.toHaveBeenCalled();
    });
});
