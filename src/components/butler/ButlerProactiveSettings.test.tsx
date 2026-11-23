import { act, cleanup, fireEvent, render, renderHook, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import ButlerProactiveSettings from "./ButlerProactiveSettings";
import { useButlerProactiveConfig } from "./useButlerProactiveConfig";
import type { ProactiveConfig } from "./proactiveApi";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@/hooks/useFilteredModels", () => ({ useFilteredModels: () => ({ models: [], loading: false, error: null }) }));

const base: ProactiveConfig = {
    enabled: true,
    private_chats: true,
    groups: true,
    normal_chats: false,
    allow_notifications: true,
    custom_instructions: "",
    pulse_hours: 3,
    daily_limit: 3,
    quiet_start: "22:30",
    quiet_end: "08:00",
    release: "08:30",
    digest: "23:00",
    small_model: "",
    deep_model: "",
    adaptive_window: false,
};

beforeEach(() => {
    invoke.mockReset();
    invoke.mockImplementation(async (cmd: string) => {
        if (cmd === "get_butler_proactive_center") return { items: [], runs: [], deliveries: [], window: { release: "09:10", digest: "23:40", days: 9 } };
        if (cmd === "get_butler_proactive_config") return { ...base };
        if (cmd === "get_assistants") return [{ id: 12, name: "Codex 助手", assistant_type: 4 }, { id: 13, name: "普通助手", assistant_type: 0 }];
        return null;
    });
});
afterEach(cleanup);

describe("ButlerProactiveSettings", () => {
    it("should show only the enable switch when proactive is off", () => {
        render(<ButlerProactiveSettings value={{ ...base, enabled: false }} onChange={vi.fn()} onOpenCenter={vi.fn()} />);
        expect(screen.getByRole("switch", { name: "启用主动整理" })).toBeInTheDocument();
        expect(screen.queryByText("整理范围")).not.toBeInTheDocument();
        expect(screen.queryByText("提醒节律与送达")).not.toBeInTheDocument();
        expect(screen.queryByRole("button", { name: /跟进事项与整理手记/ })).not.toBeInTheDocument();
    });

    it("should not render fixed ability checkboxes or separate feishu recipient input", () => {
        render(<ButlerProactiveSettings value={base} onChange={vi.fn()} onOpenCenter={vi.fn()} />);
        expect(screen.queryByText("可以做的事")).not.toBeInTheDocument();
        expect(screen.queryByLabelText("飞书提醒接收人")).not.toBeInTheDocument();
        expect(screen.getByRole("switch", { name: "主动提醒与推送" })).toBeInTheDocument();
        expect(screen.getByRole("textbox", { name: "整理偏好与关注重点" })).toBeInTheDocument();
    });

    it("should report scope and custom instructions changes through onChange", () => {
        const onChange = vi.fn();
        render(<ButlerProactiveSettings value={base} onChange={onChange} onOpenCenter={vi.fn()} />);
        fireEvent.click(screen.getByRole("checkbox", { name: "普通聊天" }));
        expect(onChange).toHaveBeenCalledWith({ ...base, normal_chats: true });
        fireEvent.change(screen.getByRole("textbox", { name: "整理偏好与关注重点" }), {
            target: { value: "重点关注项目截止日期" },
        });
        expect(onChange).toHaveBeenCalledWith({ ...base, custom_instructions: "重点关注项目截止日期" });
    });

    it("should show the interval inputs when the cadence matches no preset", () => {
        render(<ButlerProactiveSettings value={{ ...base, pulse_hours: 5 }} onChange={vi.fn()} onOpenCenter={vi.fn()} />);
        expect(screen.getByRole("spinbutton", { name: "巡检间隔（小时）" })).toHaveValue(5);
        expect(screen.getByRole("spinbutton", { name: "每日触达上限" })).toHaveValue(3);
    });

    it("should adopt the suggested activity window only when it differs and adaptive is off", async () => {
        const onChange = vi.fn();
        const { rerender } = render(<ButlerProactiveSettings value={base} onChange={onChange} onOpenCenter={vi.fn()} />);
        fireEvent.click(await screen.findByRole("button", { name: "采用建议时间" }));
        expect(onChange).toHaveBeenCalledWith({ ...base, release: "09:10", digest: "23:40" });
        rerender(<ButlerProactiveSettings value={{ ...base, adaptive_window: true }} onChange={onChange} onOpenCenter={vi.fn()} />);
        expect(screen.queryByRole("button", { name: "采用建议时间" })).not.toBeInTheDocument();
    });

    it("should open the center from the group action when enabled", () => {
        const onOpenCenter = vi.fn();
        render(<ButlerProactiveSettings value={base} onChange={vi.fn()} onOpenCenter={onOpenCenter} />);
        fireEvent.click(screen.getByRole("button", { name: /跟进事项与整理手记/ }));
        expect(onOpenCenter).toHaveBeenCalledOnce();
    });

    it("should show the load error instead of the form when config is missing", () => {
        render(<ButlerProactiveSettings error="读取主动整理设置失败：db locked" onChange={vi.fn()} onOpenCenter={vi.fn()} />);
        expect(screen.getByText("读取主动整理设置失败：db locked")).toBeInTheDocument();
        expect(screen.queryByRole("switch")).not.toBeInTheDocument();
    });
});

describe("useButlerProactiveConfig", () => {
    it("should skip saving when the config is unchanged", async () => {
        const { result } = renderHook(() => useButlerProactiveConfig());
        await waitFor(() => expect(result.current.config).toEqual(base));
        await act(() => result.current.save());
        expect(invoke).not.toHaveBeenCalledWith("save_butler_proactive_config", expect.anything());
    });

    it("should save the edited config once and treat it as the new baseline", async () => {
        const { result } = renderHook(() => useButlerProactiveConfig());
        await waitFor(() => expect(result.current.config).toEqual(base));
        act(() => result.current.setConfig({ ...base, daily_limit: 5 }));
        await act(() => result.current.save());
        await act(() => result.current.save());
        const saves = invoke.mock.calls.filter(([cmd]) => cmd === "save_butler_proactive_config");
        expect(saves).toEqual([["save_butler_proactive_config", { settings: { ...base, daily_limit: 5 } }]]);
        expect(result.current.saved?.daily_limit).toBe(5);
    });
});
