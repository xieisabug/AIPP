import { useState } from "react";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useForm, useWatch } from "react-hook-form";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import ConfigForm from "./ConfigForm";

/**
 * Mirrors forms like the data-sync settings: the parent watches a field and
 * rebuilds `config` (new objects, hints, custom renderers) on every keystroke.
 */
function WatchingHarness() {
    const form = useForm({ defaultValues: { server_url: "", token: "", note: "" } });
    const url = useWatch({ control: form.control, name: "server_url" }) || "";
    const [renders, setRenders] = useState(0);
    return (
        <ConfigForm
            title="Watching"
            useFormReturn={form}
            config={[
                {
                    key: "server_url",
                    config: {
                        type: "input",
                        label: "服务器地址",
                        placeholder: "https://sync.example.com",
                        hint: url.startsWith("http://") ? "明文传输" : undefined,
                    },
                },
                { key: "token", config: { type: "password", label: "访问 token", placeholder: "token" } },
                {
                    key: "note",
                    config: {
                        type: "custom",
                        label: "备注",
                        customRender: (field: { value?: string; onChange: (v: string) => void }) => (
                            <input
                                aria-label="备注"
                                value={field.value || ""}
                                onChange={(event) => {
                                    field.onChange(event.target.value);
                                    setRenders((n) => n + 1);
                                }}
                            />
                        ),
                    },
                },
                { key: "renders", config: { type: "static", label: "renders", value: String(renders) } },
            ]}
        />
    );
}

const viewports = [
    { name: "desktop", width: 1400 },
    { name: "mobile", width: 390 },
];

describe.each(viewports)("ConfigForm keeps focus while the parent re-renders ($name)", ({ width }) => {
    beforeEach(() => {
        window.innerWidth = width;
        window.dispatchEvent(new Event("resize"));
    });
    afterEach(() => {
        window.innerWidth = 1024;
        window.dispatchEvent(new Event("resize"));
    });

    it("types a full URL into a watched input without remounting it", async () => {
        const user = userEvent.setup();
        render(<WatchingHarness />);
        const input = screen.getByPlaceholderText("https://sync.example.com");
        await user.click(input);
        await user.type(input, "http://192.168.1.26:8080/sync");
        expect(screen.getByPlaceholderText("https://sync.example.com")).toBe(input);
        expect(document.activeElement).toBe(input);
        expect((input as HTMLInputElement).value).toBe("http://192.168.1.26:8080/sync");
        expect(screen.getByText("明文传输")).toBeTruthy();
    });

    it("keeps password and custom fields focused across re-renders", async () => {
        const user = userEvent.setup();
        render(<WatchingHarness />);
        const token = screen.getByPlaceholderText("token");
        await user.click(token);
        await user.type(token, "secret-token");
        expect(document.activeElement).toBe(token);
        expect((token as HTMLInputElement).value).toBe("secret-token");

        const note = screen.getByLabelText("备注");
        await user.click(note);
        await user.type(note, "hello");
        expect(screen.getByLabelText("备注")).toBe(note);
        expect(document.activeElement).toBe(note);
        expect((note as HTMLInputElement).value).toBe("hello");
        expect(screen.getByText("5")).toBeTruthy();
    });
});
