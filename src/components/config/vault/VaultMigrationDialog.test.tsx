import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { clearAllMockHandlers, invoke, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import VaultMigrationDialog from "./VaultMigrationDialog";

const candidate = {
    kind: "provider_key",
    configId: 4,
    field: "api_key",
    label: "Example Provider · API Key",
    target: "https://api.example.com",
    suggestedName: "Example Provider · API Key",
    suggestedKey: "example_provider_api_key",
};

describe("VaultMigrationDialog", () => {
    beforeEach(() => {
        clearAllMockHandlers();
        vi.mocked(invoke).mockClear();
        mockInvokeHandler("vault_migration_candidates", () => [candidate]);
        mockInvokeHandler("vault_migrate_candidate", () => ({ id: "entry-id", name: candidate.suggestedName }));
    });

    it("should require selecting and confirming each migration candidate", async () => {
        const user = userEvent.setup();
        const onComplete = vi.fn();
        render(<VaultMigrationDialog onComplete={onComplete} />);

        await user.click(screen.getByRole("button", { name: "迁移明文" }));
        expect(await screen.findByText(candidate.label)).toBeInTheDocument();
        expect(screen.getByText((_, element) => element?.textContent === `凭据目标：${candidate.target}`)).toBeInTheDocument();
        expect(screen.queryByText(/secret-value|api-key-value/i)).not.toBeInTheDocument();

        fireEvent.click(screen.getByRole("checkbox", { name: `迁移 ${candidate.label}` }));
        await user.click(screen.getByRole("button", { name: "确认迁移 1 项" }));

        await waitFor(() => expect(invoke).toHaveBeenCalledWith("vault_migrate_candidate", {
            candidate,
            name: candidate.suggestedName,
            key: candidate.suggestedKey,
        }));
        expect(onComplete).toHaveBeenCalledOnce();
        const serializedCalls = JSON.stringify(vi.mocked(invoke).mock.calls);
        expect(serializedCalls).not.toContain("secret-value");
    });

    it("should support select-all and clear actions for multiple candidates", async () => {
        const user = userEvent.setup();
        const second = {
            ...candidate,
            configId: 5,
            label: "Second Provider · API Key",
            suggestedName: "Second Provider · API Key",
            suggestedKey: "second_provider_api_key",
        };
        mockInvokeHandler("vault_migration_candidates", () => [candidate, second]);
        render(<VaultMigrationDialog onComplete={vi.fn()} />);

        await user.click(screen.getByRole("button", { name: "迁移明文" }));
        expect(await screen.findByText(second.label)).toBeInTheDocument();
        expect(screen.getByRole("button", { name: "确认迁移 0 项" })).toBeDisabled();

        await user.click(screen.getByRole("button", { name: "全选" }));
        expect(screen.getByRole("button", { name: "确认迁移 2 项" })).toBeEnabled();

        await user.click(screen.getByRole("button", { name: "清空" }));
        expect(screen.getByRole("button", { name: "确认迁移 0 项" })).toBeDisabled();
    });
});
