import { useState } from "react";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useForm } from "react-hook-form";
import { describe, expect, it, vi } from "vitest";

import ConfigForm from "./ConfigForm";

vi.mock("./config/FolderPicker", () => ({
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

function ConfigFormHarness() {
    const form = useForm({
        defaultValues: {
            enabled: false,
            dependent: "需要开启后显示",
        },
    });
    const [enabled, setEnabled] = useState(false);

    return (
        <ConfigForm
            title="Test Form"
            config={[
                {
                    key: "enabled",
                    config: {
                        type: "checkbox",
                        label: "启用功能",
                        value: enabled,
                        onChange: (value) => setEnabled(Boolean(value)),
                    },
                },
                {
                    key: "dependent",
                    config: {
                        type: "static",
                        label: "依赖字段",
                        value: "需要开启后显示",
                        hidden: !enabled,
                    },
                },
            ]}
            useFormReturn={form}
        />
    );
}

function FolderPickerHarness() {
    const form = useForm({
        defaultValues: {
            workingDirectory: "",
        },
    });

    return (
        <ConfigForm
            title="Folder Picker Form"
            config={[
                {
                    key: "workingDirectory",
                    config: {
                        type: "folder-picker",
                        label: "工作目录",
                        placeholder: "选择工作目录",
                    },
                },
            ]}
            useFormReturn={form}
        />
    );
}

describe("ConfigForm checkbox interactions", () => {
    it("applies custom checkbox onChange immediately", async () => {
        const user = userEvent.setup();
        render(<ConfigFormHarness />);

        const dependentField = screen.getByText("需要开启后显示").closest(".mb-6");
        expect(dependentField).toHaveClass("hidden");

        await user.click(screen.getByRole("checkbox"));

        expect(screen.getByText("需要开启后显示").closest(".mb-6")).not.toHaveClass("hidden");
    });

    it("renders the folder picker field when configured", () => {
        render(<FolderPickerHarness />);

        expect(screen.getByLabelText("选择工作目录")).toBeInTheDocument();
    });
});

function GroupedFormHarness({ assistantId = 1, hideExtensions = false, onSave = vi.fn() }) {
    const form = useForm({ defaultValues: { basic: "model", temperature: "0.7", plugin: "saved" } });
    return <ConfigForm title="Assistant" layout="prompt" useFormReturn={form}
        advancedConfigKey={assistantId} advancedFieldKeys={["temperature"]} extensionFieldKeys={["plugin"]}
        onSave={onSave} showHeaderSave={false}
        config={[
            { key: "basic", config: { type: "input", label: "基础模型" } },
            { key: "temperature", config: { type: "input", label: "温度" } },
            { key: "plugin", config: { type: "input", label: "插件参数", hidden: hideExtensions } },
        ]} />;
}

describe("ConfigForm optional sections", () => {
    it("should open the same editor when clicking the name, description or appearance", async () => {
        const user = userEvent.setup();
        const onEdit = vi.fn();
        function ProfileHarness() {
            const form = useForm();
            return <ConfigForm title="写作助手" description="帮助写作" layout="prompt" config={[]}
                useFormReturn={form} onTitleClick={onEdit} onDescriptionClick={onEdit}
                beforeFields={<button onClick={onEdit}>编辑助手形象</button>} />;
        }
        render(<ProfileHarness />);
        await user.click(screen.getByRole("button", { name: "写作助手" }));
        await user.click(screen.getByRole("button", { name: "帮助写作" }));
        await user.click(screen.getByRole("button", { name: "编辑助手形象" }));
        expect(onEdit).toHaveBeenCalledTimes(3);
        expect(screen.queryByRole("button", { name: "编辑" })).toBeNull();
    });
    it("should preserve edited and untouched values when sections are collapsed and saved", async () => {
        const user = userEvent.setup();
        const onSave = vi.fn();
        render(<GroupedFormHarness onSave={onSave} />);
        const advanced = screen.getByText("高级配置").closest("details")!;
        const extension = screen.getByText("扩展配置").closest("details")!;
        expect(advanced).not.toHaveAttribute("open");
        expect(extension).not.toHaveAttribute("open");
        expect(screen.getByLabelText("基础模型").closest("details")).toBeNull();
        await user.click(screen.getByText("扩展配置"));
        expect(extension).toHaveAttribute("open");
        await user.clear(screen.getByLabelText("插件参数"));
        await user.type(screen.getByLabelText("插件参数"), "edited");
        await user.click(screen.getByText("扩展配置"));
        await user.click(screen.getByRole("button", { name: "保存配置" }));
        expect(onSave).toHaveBeenCalledWith({ basic: "model", temperature: "0.7", plugin: "edited" });
    });

    it("should reset expanded sections when switching assistants and omit empty sections", async () => {
        const user = userEvent.setup();
        const { rerender } = render(<GroupedFormHarness />);
        await user.click(screen.getByText("高级配置"));
        expect(screen.getByText("高级配置").closest("details")).toHaveAttribute("open");
        rerender(<GroupedFormHarness assistantId={2} hideExtensions />);
        expect(screen.getByText("高级配置").closest("details")).not.toHaveAttribute("open");
        expect(screen.queryByText("扩展配置")).toBeNull();
    });
});
