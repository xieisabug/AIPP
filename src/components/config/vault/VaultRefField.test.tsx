import { useState } from "react";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { clearAllMockHandlers, invoke, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import type { VaultItem } from "@/data/Vault";
import VaultRefField from "./VaultRefField";

vi.mock("@/hooks/use-platform", () => ({ useIsMobilePlatform: () => false }));

const ID = "0f8e2a52-6c1d-4b8a-9d3e-1a2b3c4d5e6f";
const NEW_ID = "1a2b3c4d-5e6f-4a1b-8c2d-3e4f5a6b7c8d";

const item: VaultItem = {
    id: ID, handle: "github_pat", name: "GitHub 个人令牌", description: "", authKind: "bearer",
    injection: ["Authorization: Bearer <值>"], fields: [{ key: "token", label: "令牌" }], allowedSites: [], syncEnabled: true,
    status: "ready", createdAt: 1, updatedAt: 1, lastUsedAt: null,
};

interface HarnessProps {
    initial: string;
    onChange: (value: string) => void;
    available?: boolean;
    allowPrefix?: boolean;
    headerName?: string;
}

function Harness({ initial, onChange, available = true, ...rest }: HarnessProps) {
    const [value, setValue] = useState(initial);
    return (
        <VaultRefField
            vault={{ items: available ? [item] : [], available }}
            value={value}
            onChange={(next) => { setValue(next); onChange(next); }}
            ariaLabel="API Key"
            suggestedName="OpenAI · API Key"
            targetHint="https://api.openai.com"
            {...rest}
        />
    );
}

describe("VaultRefField", () => {
    beforeEach(() => {
        clearAllMockHandlers();
        vi.mocked(invoke).mockClear();
        mockInvokeHandler("vault_create", () => ({ ...item, id: NEW_ID, handle: "openai_api_key", name: "OpenAI · API Key" }));
    });

    it("shows a reference as a label and never as an editable value", () => {
        const { container } = render(<Harness initial={`vault://${ID}#token`} onChange={vi.fn()} />);
        expect(screen.getByText("GitHub 个人令牌 · 令牌")).toBeInTheDocument();
        expect(container.querySelector("input")).toBeNull();
        expect(screen.queryByText(/vault:\/\//)).not.toBeInTheDocument();
    });

    it("clears the field on remove instead of restoring the previous plain value", async () => {
        const user = userEvent.setup();
        const onChange = vi.fn();
        render(<Harness initial={`vault://${ID}#token`} onChange={onChange} />);
        await user.click(screen.getByRole("button", { name: "移除" }));
        expect(onChange).toHaveBeenLastCalledWith("");
        expect(screen.getByLabelText("API Key")).toHaveValue("");
    });

    it("stores the typed value in the vault and swaps it for a reference", async () => {
        const user = userEvent.setup();
        const onChange = vi.fn();
        render(<Harness initial="sk-live-123" onChange={onChange} />);
        await user.click(screen.getByRole("button", { name: "从凭据保险箱选择" }));
        expect(screen.getByText("保存配置后，此凭据将被确认用于 https://api.openai.com")).toBeInTheDocument();
        await user.click(screen.getByRole("radio", { name: "把当前填写的值存入保险箱" }));
        expect(screen.getByLabelText("名称")).toHaveValue("OpenAI · API Key");
        await user.click(screen.getByRole("button", { name: "确认" }));
        await waitFor(() => expect(invoke).toHaveBeenCalledWith("vault_create", {
            input: expect.objectContaining({
                name: "OpenAI · API Key",
                handle: "openai_api_key",
                auth: { kind: "bearer", token: "sk-live-123" },
                allowedSites: [],
            }),
        }));
        await waitFor(() => expect(onChange).toHaveBeenLastCalledWith(`vault://${NEW_ID}#token`));
        expect(screen.queryByDisplayValue("sk-live-123")).not.toBeInTheDocument();
    });

    it("keeps a Bearer prefix outside the stored header value", async () => {
        const user = userEvent.setup();
        const onChange = vi.fn();
        render(<Harness initial="Bearer ghp_abc" onChange={onChange} allowPrefix headerName="Authorization" />);
        await user.click(screen.getByRole("button", { name: "从凭据保险箱选择" }));
        await user.click(screen.getByRole("radio", { name: "把当前填写的值存入保险箱" }));
        await user.click(screen.getByRole("button", { name: "确认" }));
        await waitFor(() => expect(invoke).toHaveBeenCalledWith("vault_create", {
            input: expect.objectContaining({ auth: { kind: "bearer", token: "ghp_abc" } }),
        }));
        await waitFor(() => expect(onChange).toHaveBeenLastCalledWith(`Bearer vault://${NEW_ID}#token`));
        expect(screen.getByLabelText("前缀")).toHaveValue("Bearer ");
    });

    it("adds the Bearer prefix when picking a bearer credential for Authorization", async () => {
        const user = userEvent.setup();
        const onChange = vi.fn();
        render(<Harness initial="" onChange={onChange} allowPrefix headerName="Authorization" />);
        await user.click(screen.getByRole("button", { name: "从凭据保险箱选择" }));
        await user.click(screen.getByRole("button", { name: "确认" }));
        expect(onChange).toHaveBeenLastCalledWith(`Bearer vault://${ID}#token`);
        expect(invoke).not.toHaveBeenCalledWith("vault_create", expect.anything());
        expect(screen.getByText("GitHub 个人令牌 · 令牌")).toBeInTheDocument();
    });

    it("hides the vault button on devices without a vault and explains synced references", () => {
        const { unmount } = render(<Harness initial="" onChange={vi.fn()} available={false} />);
        expect(screen.queryByRole("button", { name: "从凭据保险箱选择" })).not.toBeInTheDocument();
        unmount();
        render(<Harness initial={`vault://${ID}#token`} onChange={vi.fn()} available={false} />);
        expect(screen.getByText("此设备不支持凭据保险箱")).toBeInTheDocument();
        expect(screen.queryByRole("button", { name: "更换" })).not.toBeInTheDocument();
    });

    it("marks a reference to a deleted credential", () => {
        render(<Harness initial={`vault://${NEW_ID}#token`} onChange={vi.fn()} />);
        expect(screen.getByText("凭据已删除，请重新选择")).toBeInTheDocument();
    });
});
