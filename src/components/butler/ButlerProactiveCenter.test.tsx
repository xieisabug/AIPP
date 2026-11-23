import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import ButlerProactiveCenter from "./ButlerProactiveCenter";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => undefined) }));
vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));

const item = (o: Record<string, unknown>) => ({
    kind: "followup", detail: "", tracking: "", date: undefined, revision: 1, instruction: "", source: 11, position: 4,
    yearly: false, calendar: "solar", lead: 0, ...o,
});
const content = {
    window: null,
    items: [
        item({ id: 1, title: "妈妈生日", status: "active", date: "03-14", yearly: true, calendar: "lunar", lead: 3, revision: 2 }),
        item({ id: 2, title: "整理季度报销单", status: "dismissed" }),
    ],
    runs: [{ id: "r1", trigger: "manual", status: "failed", summary: "初筛模型调用失败：HTTP 429", scanned: 1, time: 1_700_000_000 }],
    deliveries: [],
};

beforeEach(() => {
    invoke.mockReset();
    invoke.mockImplementation(async (cmd: string) => cmd === "get_butler_proactive_center" ? structuredClone(content) : null);
});
afterEach(cleanup);

it("should list active items with their timing and keep closed ones folded", async () => {
    render(<ButlerProactiveCenter open onOpenChange={vi.fn()} enabled />);
    expect(await screen.findByText("妈妈生日")).toBeInTheDocument();
    expect(screen.getByText("03-14 · 农历 · 每年 · 提前 3 天提醒")).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "跟进事项 1" })).toBeInTheDocument();
    expect(screen.getByText("已完成或关闭（1）")).toBeInTheDocument();
});

it("should dismiss an active item with its current revision", async () => {
    render(<ButlerProactiveCenter open onOpenChange={vi.fn()} enabled />);
    fireEvent.click(await screen.findByRole("button", { name: "不再跟进" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("update_butler_proactive_item",
        expect.objectContaining({ id: 1, revision: 2, action: "dismiss" })));
});

it("should disable running now while proactive is off", async () => {
    render(<ButlerProactiveCenter open onOpenChange={vi.fn()} enabled={false} />);
    await screen.findByText("妈妈生日");
    expect(screen.getByRole("button", { name: "立即整理一次" })).toBeDisabled();
});

it("should show the load failure reason", async () => {
    invoke.mockRejectedValue("database is locked");
    render(<ButlerProactiveCenter open onOpenChange={vi.fn()} enabled />);
    expect(await screen.findByRole("alert")).toHaveTextContent("读取主动整理内容失败：database is locked");
});
