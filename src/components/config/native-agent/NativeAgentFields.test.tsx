import React from "react";
import { fireEvent, render, renderHook, screen, waitFor } from "@testing-library/react";
import { useForm } from "react-hook-form";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { DetectedProgram, NativeAgentEnvironment, NativeAgentEnvironmentState } from "@/hooks/feature/useNativeAgentEnvironment";
import { AgentProgramField, CursorSdkField, NodeRuntimeField, useNativeAgentProviderFields } from "./NativeAgentFields";
import { samePath } from "./ProgramPicker";

const mocks = vi.hoisted(() => ({ open: vi.fn(), toastSuccess: vi.fn(), toastError: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: mocks.open }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), Channel: class {} }));
vi.mock("sonner", () => ({ toast: { success: mocks.toastSuccess, error: mocks.toastError } }));
vi.mock("@/components/config/AntigravityLogin", () => ({ default: () => null }));
// Radix Select renders through a portal with pointer heuristics; a button list keeps tests about our logic.
vi.mock("@/components/ui/select", () => {
    const Context = React.createContext<(value: string) => void>(() => {});
    return {
        Select: ({ value, onValueChange, children }: { value: string; onValueChange: (value: string) => void; children: React.ReactNode }) =>
            <div data-testid="select" data-value={value}><Context.Provider value={onValueChange}>{children}</Context.Provider></div>,
        SelectTrigger: ({ children, ...props }: { children: React.ReactNode }) => <div role="combobox" {...props}>{children}</div>,
        SelectValue: ({ children }: { children: React.ReactNode }) => <span>{children}</span>,
        SelectContent: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
        SelectItem: ({ value, children }: { value: string; children: React.ReactNode }) => {
            const choose = React.useContext(Context);
            return <button type="button" role="option" data-value={value} onClick={() => choose(value)}>{children}</button>;
        },
    };
});

const program = (overrides: Partial<DetectedProgram>): DetectedProgram => ({ path: "", source: "PATH", version: null, ok: true, warning: null, error: null, ...overrides });

function environment(overrides: Partial<NativeAgentEnvironment> = {}): NativeAgentEnvironment {
    return {
        driver: "pi",
        node: {
            configured: "",
            required: "22.19.0",
            selected: program({ path: "C:\\nvm4w\\nodejs\\node.exe", version: "25.9.0" }),
            candidates: [
                program({ path: "C:\\nvm4w\\nodejs\\node.exe", version: "25.9.0" }),
                program({ path: "D:\\nvm\\v22.14.0\\node.exe", source: "nvm", version: "22.14.0", warning: "低于要求的 Node.js 22.19.0" }),
            ],
        },
        tool: {
            kind: "cli", display_name: "Pi", default_command: "pi", configured: "",
            selected: program({ path: "C:\\Users\\u\\.pi\\agent\\bin\\pi.ps1", source: "Pi 安装器", version: "1.5.1" }),
            candidates: [program({ path: "C:\\Users\\u\\.pi\\agent\\bin\\pi.ps1", source: "Pi 安装器", version: "1.5.1" })],
            install_commands: [{ label: "npm", command: "npm install -g @earendil-works/pi-coding-agent" }],
            docs_url: "https://pi.dev", can_update: true, managed_directory: null, warnings: [], account: null, uses_api_key: false,
        },
        working_directory: { configured: "", resolved: "C:\\Users\\u", exists: true },
        ...overrides,
    };
}

function state(overrides: Partial<NativeAgentEnvironmentState> = {}): NativeAgentEnvironmentState {
    const env = overrides.environment ?? environment();
    return {
        environment: env, detecting: false, detectError: null, detect: vi.fn(),
        currentVersion: env.tool.selected?.ok ? env.tool.selected.version : null,
        updateInfo: null, checkingUpdate: false, checkError: null, checkUpdate: vi.fn(),
        updating: false, updateLog: "", updateError: null, runUpdate: vi.fn().mockResolvedValue(null),
        ...overrides,
    };
}

describe("native agent provider fields", () => {
    beforeEach(() => { mocks.open.mockReset(); mocks.toastSuccess.mockReset(); mocks.toastError.mockReset(); });

    it("should store an empty value for automatic lookup and the exact path when a detected Node is chosen", () => {
        const onChange = vi.fn();
        render(<NodeRuntimeField state={state()} value="node" onChange={onChange} />);
        expect(screen.getByTestId("select")).toHaveAttribute("data-value", "__auto__");
        expect(screen.getByRole("combobox")).toHaveTextContent("自动 · v25.9.0");
        fireEvent.click(screen.getByRole("option", { name: /v22.14.0 · nvm/ }));
        expect(onChange).toHaveBeenCalledWith("D:\\nvm\\v22.14.0\\node.exe");
        fireEvent.click(screen.getByRole("option", { name: /自动（PATH 中的 node）/ }));
        expect(onChange).toHaveBeenLastCalledWith("");
    });

    it("should offer a supported Node when the selected runtime is older than required", () => {
        const env = environment();
        env.node.selected = env.node.candidates[1];
        env.node.candidates = [env.node.candidates[1], program({ path: "D:\\nvm\\v25.9.0\\node.exe", source: "nvm", version: "25.9.0" })];
        const onChange = vi.fn();
        render(<NodeRuntimeField state={state({ environment: env })} value={"D:\\nvm\\v22.14.0\\node.exe"} onChange={onChange} />);
        expect(screen.getByText("低于要求的 Node.js 22.19.0")).toBeInTheDocument();
        fireEvent.click(screen.getByRole("button", { name: "改用此版本" }));
        expect(onChange).toHaveBeenCalledWith("D:\\nvm\\v25.9.0\\node.exe");
    });

    it("should show the custom path editor and browse for a file when the stored path is not a detected install", async () => {
        mocks.open.mockResolvedValue("E:\\tools\\pi.js");
        const onChange = vi.fn();
        render(<AgentProgramField state={state()} value={"E:\\old\\pi.js"} onChange={onChange} />);
        expect(screen.getByTestId("select")).toHaveAttribute("data-value", "__custom__");
        expect(screen.getByDisplayValue("E:\\old\\pi.js")).toBeInTheDocument();
        fireEvent.click(screen.getByRole("button", { name: "选择 Pi 程序" }));
        await waitFor(() => expect(onChange).toHaveBeenCalledWith("E:\\tools\\pi.js"));
        expect(mocks.open).toHaveBeenCalledWith(expect.objectContaining({ directory: false, multiple: false }));
    });

    it("should show copyable install commands when the agent program is not found", () => {
        const env = environment();
        env.tool.selected = program({ path: "pi", ok: false, error: "未找到 pi" });
        env.tool.candidates = [];
        env.tool.can_update = false;
        render(<AgentProgramField state={state({ environment: env, currentVersion: null })} value="" onChange={vi.fn()} />);
        expect(screen.getAllByText("未找到 pi").length).toBeGreaterThan(0);
        expect(screen.getByText("npm install -g @earendil-works/pi-coding-agent")).toBeInTheDocument();
        expect(screen.queryByText("检查更新")).not.toBeInTheDocument();
    });

    it("should offer a one-click update when a newer release is published", async () => {
        const runUpdate = vi.fn().mockResolvedValue(null);
        render(<AgentProgramField state={state({ updateInfo: { latest: "1.5.2", update_available: true, source: "npm" }, runUpdate })} value="" onChange={vi.fn()} />);
        expect(screen.getByText(/新版本 v1.5.2 可用/)).toBeInTheDocument();
        fireEvent.click(screen.getByRole("button", { name: "更新" }));
        await waitFor(() => expect(mocks.toastSuccess).toHaveBeenCalledWith("Pi 已更新"));
        expect(runUpdate).toHaveBeenCalledWith(false);
    });

    it("should keep the updater error visible with retry choices when an update failed", () => {
        render(<AgentProgramField state={state({ updateError: "Pi 更新失败（pi update self，退出码 1）：EACCES", updateLog: "npm ERR! EACCES" })} value="" onChange={vi.fn()} />);
        expect(screen.getByText(/退出码 1/)).toBeInTheDocument();
        expect(screen.getByText("npm ERR! EACCES")).toBeInTheDocument();
        expect(screen.getByRole("button", { name: "使用全局代理重试" })).toBeInTheDocument();
    });

    it("should install the Cursor SDK into the managed directory and pin it when none is found", async () => {
        const env = environment({ driver: "cursor" });
        env.tool = { ...env.tool, kind: "sdk", display_name: "Cursor SDK", default_command: "", can_update: true,
            selected: program({ path: "C:\\Users\\u", source: "工作目录", ok: false, error: "C:\\Users\\u 中没有安装 @cursor/sdk" }),
            candidates: [], managed_directory: "C:\\AppData\\aipp\\agent-runtimes\\cursor-sdk",
            warnings: ["未配置 Cursor API Key：填写下方 API Key，或在环境变量中设置 CURSOR_API_KEY"] };
        const runUpdate = vi.fn().mockResolvedValue("C:\\AppData\\aipp\\agent-runtimes\\cursor-sdk");
        const onChange = vi.fn();
        render(<CursorSdkField state={state({ environment: env, currentVersion: null, runUpdate })} value="" onChange={onChange} />);
        expect(screen.getByText(/未配置 Cursor API Key/)).toBeInTheDocument();
        expect(screen.getByText(/AIPP 管理目录（C:\\AppData\\aipp\\agent-runtimes\\cursor-sdk）/)).toBeInTheDocument();
        fireEvent.click(screen.getByRole("button", { name: /一键安装 Cursor SDK/ }));
        await waitFor(() => expect(onChange).toHaveBeenCalledWith("C:\\AppData\\aipp\\agent-runtimes\\cursor-sdk"));
        expect(mocks.toastSuccess).toHaveBeenCalledWith("Cursor SDK 已安装");
    });

    it("should suggest the managed SDK when another directory is selected without it", () => {
        const env = environment({ driver: "cursor" });
        env.tool = { ...env.tool, kind: "sdk", display_name: "Cursor SDK", default_command: "",
            selected: program({ path: "E:\\project", source: "所选目录", ok: false, error: "E:\\project 中没有安装 @cursor/sdk" }),
            candidates: [program({ path: "C:\\AppData\\cursor-sdk", source: "AIPP 管理", version: "1.3.1" })] };
        const onChange = vi.fn();
        render(<CursorSdkField state={state({ environment: env })} value={"E:\\project"} onChange={onChange} />);
        fireEvent.click(screen.getByRole("button", { name: "使用此目录" }));
        expect(onChange).toHaveBeenCalledWith("C:\\AppData\\cursor-sdk");
        expect(screen.queryByRole("button", { name: /一键安装/ })).not.toBeInTheDocument();
    });
});

describe("samePath", () => {
    it("should compare Windows paths case- and slash-insensitively when matching stored values", () => {
        expect(samePath("C:\\Users\\U\\pi.ps1", "c:/users/u/pi.ps1")).toBe(true);
        expect(samePath("/opt/Pi/bin/pi", "/opt/pi/bin/pi")).toBe(false);
        expect(samePath("/opt/pi/", "/opt/pi")).toBe(true);
    });
});

describe("useNativeAgentProviderFields", () => {
    const keys = (apiType: string, values: Record<string, string> = {}) => {
        const { result } = renderHook(() => {
            const form = useForm({ defaultValues: values });
            return useNativeAgentProviderFields({ apiType, apiTypeLabel: apiType, providerId: "3", form });
        });
        return result.current?.map((field) => field.key) ?? null;
    };

    it("should leave other provider types to their own forms", () => {
        expect(keys("openai_api")).toBeNull();
        expect(keys("codex_app_server")).toBeNull();
    });

    it("should show only the fields that apply to the chosen Antigravity runtime and authentication", () => {
        const cli = keys("antigravity");
        expect(cli).toEqual(expect.arrayContaining(["agent_node_command", "antigravity_runtime", "agent_cli_command", "antigravity_auth_method", "antigravity_login", "acp_working_directory"]));
        expect(cli).not.toContain("antigravity_harness");
        expect(cli).not.toContain("antigravity_api_key");
        const standalone = keys("antigravity", { antigravity_runtime: "standalone", antigravity_auth_method: "gemini-api-key" });
        expect(standalone).toEqual(expect.arrayContaining(["antigravity_harness", "antigravity_api_key"]));
    });

    it("should replace the CLI picker with SDK and API key fields for Cursor", () => {
        const cursor = keys("cursor");
        expect(cursor).toEqual(expect.arrayContaining(["agent_sdk_directory", "cursor_api_key"]));
        expect(cursor).not.toContain("agent_cli_command");
        expect(keys("kiro")).toEqual(expect.arrayContaining(["agent_cli_command", "kiro_agent", "kiro_effort"]));
    });
});

describe("AgentProgramField without Node", () => {
    it("should not claim the agent is missing when Node is unavailable to verify it", () => {
        const env = environment();
        env.node.selected = null;
        env.tool.selected = program({ path: "pi", source: "未检测", ok: false, error: "需要先选择可运行的 Node.js（≥ 22.19.0）才能检测 Pi" });
        env.tool.candidates = [program({ path: "/home/u/.pi/agent/bin/pi", source: "Pi 安装器", ok: false })];
        render(<AgentProgramField state={state({ environment: env, currentVersion: null })} value="" onChange={vi.fn()} />);
        expect(screen.getAllByText(/需要先选择可运行的 Node.js/).length).toBeGreaterThan(0);
        expect(screen.queryByText(/npm install -g/)).not.toBeInTheDocument();
    });
});
