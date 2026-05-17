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
