import { act, render, screen, waitFor } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import { beforeEach, it, expect } from "vitest";
import { mockInvokeHandler, clearAllMockHandlers } from "@/__tests__/mocks/tauri";
import { type SetupItem, OnboardingSetupChip, SetupList, useSetupItems } from "./OnboardingSetup";

function StatusList() { return <SetupList items={useSetupItems()}/>; }
beforeEach(()=>clearAllMockHandlers());
it("should retain Office progress received before the initial task status resolves", async () => {
    let resolveStatus!: (items: SetupItem[]) => void;
    mockInvokeHandler("get_onboarding_setup_status", () => new Promise<SetupItem[]>(resolve => {
        resolveStatus = resolve;
    }));
    render(<StatusList />);
    await act(() => emit("plugin_install_progress", {
        pluginCode: "office-preview-plugin", stage: "downloading", percent: 42,
    }));
    expect(screen.queryByText(/正在下载 42%/)).not.toBeInTheDocument();
    await act(async () => resolveStatus([{ id: "office", state: "running", message: "" }]));
    expect(await screen.findByText(/正在下载 42%/)).toBeVisible();
});
it("should display live Office download percentages only while the durable task is running",async()=>{
    let item:SetupItem={id:"office",state:"running",message:""};
    mockInvokeHandler("get_onboarding_setup_status",()=>[item]);
    render(<StatusList/>); await screen.findByText("Office 插件");
    await act(async()=>{await emit("plugin_install_progress",{pluginCode:"office-preview-plugin",stage:"downloading",percent:34,message:""});});
    expect(await screen.findByText(/正在下载 34%/)).toBeInTheDocument();
    item={id:"office",state:"failed",message:"网络连接中断"};
    await act(async()=>{await emit("onboarding-setup-progress",[item]);});
    await screen.findByText(/网络连接中断/);
    expect(screen.getByRole("button",{name:"重试"})).toBeInTheDocument();
    await act(async()=>{await emit("plugin_install_progress",{pluginCode:"office-preview-plugin",stage:"downloading",percent:80,message:""});});
    await waitFor(()=>expect(screen.queryByText(/80%/)).not.toBeInTheDocument());
});
it("should stay hidden on mount when setup is already complete and only show 准备好了 after active work finishes", async () => {
    let items: SetupItem[] = [{ id: "bun", state: "done", message: "已安装" }];
    mockInvokeHandler("get_onboarding_setup_status", () => items);
    const { unmount } = render(<OnboardingSetupChip />);
    await waitFor(() => expect(screen.queryByText("准备好了")).not.toBeInTheDocument());
    unmount();

    items = [{ id: "bun", state: "running", message: "" }];
    render(<OnboardingSetupChip />);
    expect(await screen.findByText("正在准备 0/1")).toBeInTheDocument();
    items = [{ id: "bun", state: "done", message: "已安装" }];
    await act(async () => { await emit("onboarding-setup-progress", items); });
    expect(await screen.findByText("准备好了")).toBeInTheDocument();
});
