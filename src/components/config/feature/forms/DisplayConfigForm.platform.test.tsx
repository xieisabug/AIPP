import { render, screen, waitFor } from "@testing-library/react";
import { useForm } from "react-hook-form";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { DisplayConfigForm, getDisplayConfigDescription } from "./DisplayConfigForm";

const platformState = vi.hoisted(() => ({ mobile: false }));
vi.mock("@/hooks/use-platform", () => ({
    useIsMobilePlatform: () => platformState.mobile,
}));
vi.mock("@/hooks/highlight/useSyntectThemes", () => ({
    useSyntectThemes: () => ({ themes: [], themeInfo: [] }),
}));
vi.mock("@/services/PluginRuntime", () => ({
    pluginRuntime: {
        loadPlugins: vi.fn(async () => undefined),
        listDisplayThemes: vi.fn(async () => []),
    },
}));
vi.mock("@/services/builtinMcpToolComponents", () => ({
    ensureBuiltinMcpToolComponentsRegistered: vi.fn(),
}));

function Harness() {
    const form = useForm<any>({
        defaultValues: {
            default_home_window: "chat",
            theme: "default",
            color_mode: "system",
            code_theme_light: "",
            code_theme_dark: "",
            user_message_markdown_render: "enabled",
            notification_on_completion: false,
            merge_assistant_messages: true,
            show_thinking: true,
            preview_code_show_toolbar: false,
            mcp_tool_call_component_id: "",
        },
    });
    return <DisplayConfigForm form={form as any} onSave={async () => undefined} showButlerHomeWindow={false} />;
}

describe("DisplayConfigForm subtitle per platform", () => {
    beforeEach(() => {
        platformState.mobile = false;
    });

    it("desktop keeps the 默认主页窗口 row and mentions it in the subtitle", async () => {
        render(<Harness />);
        await waitFor(() => expect(screen.getByText("系统外观主题")).toBeTruthy());
        expect(screen.getByText("默认主页窗口")).toBeTruthy();
        expect(screen.getByText("配置默认主页窗口、系统外观主题、深浅色模式和用户消息渲染方式")).toBeTruthy();
    });

    it("mobile hides the 默认主页窗口 row and the subtitle no longer mentions it", async () => {
        platformState.mobile = true;
        const { container } = render(<Harness />);
        await waitFor(() => expect(screen.getByText("系统外观主题")).toBeTruthy());
        expect(screen.queryByText("默认主页窗口")).toBeNull();
        expect(screen.getByText("配置外观主题、深浅色模式和消息渲染方式")).toBeTruthy();
        expect(container.textContent).not.toContain("默认主页窗口");
    });

    it("getDisplayConfigDescription only names options the platform shows", () => {
        expect(getDisplayConfigDescription(false)).toContain("默认主页窗口");
        expect(getDisplayConfigDescription(true)).not.toContain("默认主页窗口");
        expect(getDisplayConfigDescription(true)).toContain("深浅色模式");
    });
});
