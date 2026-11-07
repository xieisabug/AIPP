import { render, screen } from "@testing-library/react";
import { useForm } from "react-hook-form";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";

import { ExperimentalConfigForm } from "./ExperimentalConfigForm";
import {
    EXPERIMENTAL_CONFIG_DEFAULT_VALUES,
    ExperimentalConfigFormState,
} from "./experimentalConfigShared";

vi.mock("@/hooks/useModels", () => ({
    useModels: () => ({
        models: [],
        loading: false,
        error: null,
    }),
}));

vi.mock("@/components/config/FolderPicker", () => ({
    FolderPicker: ({
        value,
        onChange,
        placeholder,
    }: {
        value: string;
        onChange: (value: string) => void;
        placeholder?: string;
    }) => (
        <input
            aria-label={placeholder || "folder-picker"}
            value={value}
            onChange={(event) => onChange(event.target.value)}
        />
    ),
}));

function ButlerScopeHarness({ enabled = false }: { enabled?: boolean } = {}) {
    const form = useForm<ExperimentalConfigFormState>({
        defaultValues: { ...EXPERIMENTAL_CONFIG_DEFAULT_VALUES, butler_experiment_enabled: String(enabled) },
    });

    return (
        <ExperimentalConfigForm
            form={form}
            onSave={async () => undefined}
            scope="butler"
        />
    );
}

describe("ExperimentalConfigForm", () => {
    beforeEach(() => {
        mockInvokeHandler("get_experimental_summary_task_status", () => ({
            mcp_running: false,
            assistant_running: false,
            conversation_running: false,
            conversation_running_count: 0,
        }));
        mockInvokeHandler("get_butler_feishu_runtime_status", () => ({
            butler_enabled: false,
            enabled: false,
            configured: false,
            secret_configured: false,
            running: false,
            connected: false,
            allow_p2p: true,
            allow_group: true,
            group_require_mention: true,
            status_text: "未启用",
        }));
    });

    afterEach(() => {
        clearAllMockHandlers();
        vi.clearAllMocks();
    });

    it("在 Butler 模式下只显示管家相关实验设置", () => {
        render(<ButlerScopeHarness />);

        expect(screen.getByText("总管家与飞书接入")).toBeInTheDocument();
        expect(screen.getByRole("button", { name: "保存配置" })).toBeInTheDocument();
        expect(screen.queryByText("实验性功能")).not.toBeInTheDocument();
        expect(screen.queryByText("摘要与动态加载")).not.toBeInTheDocument();
        expect(screen.queryByText("MCP 动态加载（实验）")).not.toBeInTheDocument();
    });
    it("should expose independent rollback switches without claiming to cancel external work", () => {
        render(<ButlerScopeHarness enabled />);
        expect(screen.getByText("管家能力开关")).toBeInTheDocument();
        for (const label of ["新建独立任务", "发起成员协作", "继续执行已有工作", "后台检查"]) {
            expect(screen.getByText(label)).toBeInTheDocument();
        }
        expect(screen.getByText(/关闭不会撤销已发生的外部操作/)).toBeInTheDocument();
    });

});
