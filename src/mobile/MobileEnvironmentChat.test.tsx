import { Suspense, type ReactNode } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import MobileEnvironmentChat from "./MobileEnvironmentChat";
import type { Message } from "@/data/Conversation";
vi.mock("@/components/conversation/InputArea", () => ({default: () => null}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../windows/ChatUIWindow", () => ({ default: ({ mobileEnvironmentSwitch }: { mobileEnvironmentSwitch: ReactNode }) =>
    <div>本机对话{mobileEnvironmentSwitch}</div> }));
vi.mock("@/components/conversation/VirtuosoMessageList", () => ({ default: ({ allDisplayMessages }: { allDisplayMessages: Message[] }) =>
    <div>{allDisplayMessages.map(m => <p key={m.id}>{m.content}</p>)}</div> }));
vi.mock("@/components/MessageItem", () => ({ default: () => null }));
const home = { id: "a".repeat(64), name: "家里电脑" };
const office = { id: "b".repeat(64), name: "办公室电脑" };
const directory = { protocolVersion: 1, capabilities: ["conversation_history"], butler: true,
    conversations: [{ id: 7, name: "测试对话", kind: "normal" }, { id: 8, name: "总管家主会话", kind: "butler_main" },
        { id: 9, name: "小助手私聊", kind: "butler_contact" }] };
function renderChat() { return render(<Suspense><MobileEnvironmentChat /></Suspense>); }
async function select(name: string) {
    fireEvent.click(await screen.findByRole("button", { name: /切换运行环境/ }));
    fireEvent.click(await screen.findByRole("button", { name }));
}

describe("mobile runtime environment", () => {
    beforeEach(() => {
        const storage = new Map<string,string>();
        vi.stubGlobal("localStorage", {
            getItem: (key: string) => storage.get(key) ?? null,
            setItem: (key: string, value: string) => storage.set(key,value),
            removeItem: (key: string) => storage.delete(key),
        });
        vi.mocked(invoke).mockReset();
        vi.mocked(invoke).mockImplementation(async (command, args: any) => {
            if (command === "bridge_environments") return { hosts: [home, office] };
            if (command === "bridge_connect") return args.conversation
                ? { hasMore: false, messages: [{ id: 1, role: "response", content: args.id === home.id ? "家里的对话正文" : "办公室的对话正文" }] }
                : directory;
            throw Error(String(command));
        });
    });
    it("should restore the selected conversation per host when both computers use the same local ID", async () => {
        renderChat();
        await select(home.name);
        fireEvent.click(await screen.findByText("测试对话"));
        expect(await screen.findByText("家里的对话正文")).toBeVisible();
        await select(office.name);
        fireEvent.click(await screen.findByText("测试对话"));
        expect(await screen.findByText("办公室的对话正文")).toBeVisible();
        await select(home.name);
        expect(await screen.findByText("家里的对话正文")).toBeVisible();
        expect(screen.queryByText("办公室的对话正文")).toBeNull();
    });
    it("should keep an offline computer selected without invoking local execution", async () => {
        localStorage.setItem("aipp.mobile.environment", JSON.stringify(home));
        vi.mocked(invoke).mockRejectedValue(new Error("电脑连接超时"));
        renderChat();
        expect(await screen.findByRole("alert")).toHaveTextContent("电脑连接超时");
        expect(screen.queryByText("本机对话")).toBeNull();
        expect(invoke).toHaveBeenCalledWith("bridge_connect", { id: home.id });
    });
    it("should keep Butler and contact selection on the active host", async () => {
        localStorage.setItem("aipp.mobile.environment", JSON.stringify(office));
        renderChat();
        await screen.findByText("测试对话");
        fireEvent.click(screen.getByRole("button", { name: "管家" }));
        expect(screen.getByText("总管家主会话")).toBeVisible();
        fireEvent.click(screen.getByText("小助手私聊"));
        await waitFor(() => expect(invoke).toHaveBeenCalledWith("bridge_connect", { id: office.id, conversation: 9, before: undefined }));
        expect(screen.queryByText("本机对话")).toBeNull();
    });
    it("should reload grants when pairing the selected desktop again", async () => {
        localStorage.setItem("aipp.mobile.environment", JSON.stringify(home));
        let granted = false;
        vi.mocked(invoke).mockImplementation(async (command) => {
            if (command === "bridge_environments") return { hosts:[home] };
            if (command === "bridge_pair_phone") { granted = true; return {...home}; }
            return {...directory,capabilities:granted ? ["conversation_history","chat"] : ["conversation_history"],
                assistants:[{id:1,name:"验收助手"}]};
        });
        renderChat();
        await screen.findByText(/当前仅可查看历史/);
        fireEvent.click(screen.getByRole("button",{name:/切换运行环境/}));
        fireEvent.click(await screen.findByRole("button",{name:"扫码配对电脑"}));
        expect(await screen.findByRole("button",{name:"与验收助手新建对话"})).toBeVisible();
    });
});
