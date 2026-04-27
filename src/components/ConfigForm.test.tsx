import { useState } from "react";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useForm } from "react-hook-form";
import { describe, expect, it } from "vitest";

import ConfigForm from "./ConfigForm";

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

describe("ConfigForm checkbox interactions", () => {
    it("applies custom checkbox onChange immediately", async () => {
        const user = userEvent.setup();
        render(<ConfigFormHarness />);

        const dependentField = screen.getByText("需要开启后显示").closest(".mb-6");
        expect(dependentField).toHaveClass("hidden");

        await user.click(screen.getByRole("checkbox"));

        expect(screen.getByText("需要开启后显示").closest(".mb-6")).not.toHaveClass("hidden");
    });
});
