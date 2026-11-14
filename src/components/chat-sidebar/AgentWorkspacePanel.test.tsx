import React from "react";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import AgentWorkspacePanel from "./AgentWorkspacePanel";
import type { AgentGitBranch, AgentGitStatus } from "@/data/AgentWorkspace";
import { clearAllMockHandlers, invoke, mockInvokeHandler } from "@/__tests__/mocks/tauri";

const toastMock = vi.hoisted(() => ({ success: vi.fn(), error: vi.fn() }));
vi.mock("sonner", () => ({ toast: toastMock }));

// Radix Popover / cmdk 在 happy-dom 里依赖 portal 与布局 API，这里换成直接渲染的简化版本
vi.mock("@/components/ui/popover", () => {
    const PopoverContext = React.createContext<{ open: boolean; onOpenChange: (open: boolean) => void }>({
        open: false,
        onOpenChange: () => {},
    });
    return {
        Popover: ({ open, onOpenChange, children }: { open: boolean; onOpenChange: (open: boolean) => void; children: React.ReactNode }) => (
            <PopoverContext.Provider value={{ open, onOpenChange }}>{children}</PopoverContext.Provider>
        ),
        PopoverTrigger: ({ children }: { children: React.ReactElement<{ onClick?: () => void }> }) => {
            const { open, onOpenChange } = React.useContext(PopoverContext);
            return React.cloneElement(children, { onClick: () => onOpenChange(!open) });
        },
        PopoverContent: ({ children }: { children: React.ReactNode }) => {
            const { open } = React.useContext(PopoverContext);
            return open ? <div data-testid="popover-content">{children}</div> : null;
        },
    };
});
vi.mock("@/components/ui/command", () => ({
    Command: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
    CommandInput: ({ value, onValueChange, placeholder }: { value: string; onValueChange: (value: string) => void; placeholder?: string }) => (
        <input aria-label={placeholder} value={value} onChange={(event) => onValueChange(event.target.value)} />
    ),
    CommandList: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
    CommandEmpty: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
    CommandGroup: ({ heading, children }: { heading: string; children: React.ReactNode }) => (
        <div role="group" aria-label={heading}>{children}</div>
    ),
    CommandItem: ({ children, onSelect, disabled }: { children: React.ReactNode; onSelect: () => void; disabled?: boolean }) => (
        <button type="button" role="option" disabled={disabled} onClick={onSelect}>{children}</button>
    ),
}));

const repoStatus = (overrides: Partial<AgentGitStatus> = {}): AgentGitStatus => ({
    working_directory: "E:/repo",
    is_repo: true,
    branch: "main",
    detached: false,
    upstream: "origin/main",
    ahead: 0,
    behind: 0,
    has_remote: true,
    has_uncommitted_changes: true,
    diff: { base: "origin/main", files_changed: 3, insertions: 1331, deletions: 37, untracked_files: 1 },
    ...overrides,
});

const branch = (name: string, isRemote = false, isCurrent = false): AgentGitBranch => ({
    name,
    full_ref: isRemote ? `refs/remotes/${name}` : `refs/heads/${name}`,
    is_remote: isRemote,
    is_current: isCurrent,
    upstream: null,
    committed_at: null,
});

function renderPanel(props: Partial<React.ComponentProps<typeof AgentWorkspacePanel>> = {}) {
    const onCompact = vi.fn(async () => {});
    const onGenerateCommitMessage = vi.fn(async () => "feat: generated");
    const utils = render(
        <AgentWorkspacePanel
            conversationId="42"
            compactSupported
            busy={false}
            onCompact={onCompact}
            onGenerateCommitMessage={onGenerateCommitMessage}
            {...props}
        />,
    );
    return { ...utils, onCompact, onGenerateCommitMessage };
}

describe("AgentWorkspacePanel", () => {
    beforeEach(() => {
        clearAllMockHandlers();
        invoke.mockClear();
        toastMock.success.mockReset();
        toastMock.error.mockReset();
    });

    it("should show branch, commit, diff stats and compact rows for a git repo", async () => {
        mockInvokeHandler("get_agent_git_status", () => repoStatus());
        renderPanel();

        expect(await screen.findByText("main")).toBeInTheDocument();
        expect(screen.getByText("提交与推送")).toBeInTheDocument();
        expect(screen.getByText("对比 origin/main")).toBeInTheDocument();
        expect(screen.getByText("+1,331")).toBeInTheDocument();
        expect(screen.getByText("−37")).toBeInTheDocument();
        expect(screen.getByRole("button", { name: /压缩/ })).toBeEnabled();
        expect(invoke).toHaveBeenCalledWith("get_agent_git_status", { conversationId: 42 });
    });

    it("should only show the compact row when the working directory is not a git repo", async () => {
        mockInvokeHandler("get_agent_git_status", () => repoStatus({ is_repo: false, branch: null }));
        renderPanel();

        await waitFor(() => expect(invoke).toHaveBeenCalledWith("get_agent_git_status", { conversationId: 42 }));
        expect(screen.getByRole("button", { name: /压缩/ })).toBeInTheDocument();
        expect(screen.queryByText("main")).not.toBeInTheDocument();
        expect(screen.queryByText(/改动/)).not.toBeInTheDocument();
    });

    it("should surface the git status error with a retry", async () => {
        let calls = 0;
        mockInvokeHandler("get_agent_git_status", () => {
            calls += 1;
            if (calls === 1) throw new Error("未找到 git 可执行文件");
            return repoStatus();
        });
        const user = userEvent.setup();
        renderPanel();

        expect(await screen.findByText(/Git 状态读取失败：未找到 git 可执行文件/)).toBeInTheDocument();
        await user.click(screen.getByRole("button", { name: "重试读取 Git 状态" }));
        expect(await screen.findByText("main")).toBeInTheDocument();
    });

    it("should call onCompact and disable compact when the agent does not support it", async () => {
        mockInvokeHandler("get_agent_git_status", () => repoStatus());
        const user = userEvent.setup();
        const { onCompact, rerender } = renderPanel();
        await screen.findByText("main");

        await user.click(screen.getByRole("button", { name: /压缩/ }));
        expect(onCompact).toHaveBeenCalledOnce();

        rerender(
            <AgentWorkspacePanel
                conversationId="42"
                compactSupported={false}
                busy={false}
                onCompact={onCompact}
                onGenerateCommitMessage={async () => ""}
            />,
        );
        const compact = screen.getByRole("button", { name: /压缩/ });
        expect(compact).toBeDisabled();
        expect(compact).toHaveTextContent("不支持");
    });

    it("should disable branch switching and compact while the agent is responding", async () => {
        mockInvokeHandler("get_agent_git_status", () => repoStatus());
        renderPanel({ busy: true });
        await screen.findByText("main");

        expect(screen.getByText("main").closest("button")).toBeDisabled();
        expect(screen.getByRole("button", { name: /压缩/ })).toBeDisabled();
    });

    it("should refresh git status when the agent finishes a turn", async () => {
        mockInvokeHandler("get_agent_git_status", () => repoStatus());
        const props = { conversationId: "42", compactSupported: true, onCompact: async () => {}, onGenerateCommitMessage: async () => "" };
        const { rerender } = render(<AgentWorkspacePanel {...props} busy />);
        await screen.findByText("main");
        const before = invoke.mock.calls.filter(([cmd]) => cmd === "get_agent_git_status").length;

        rerender(<AgentWorkspacePanel {...props} busy={false} />);
        await waitFor(() =>
            expect(invoke.mock.calls.filter(([cmd]) => cmd === "get_agent_git_status").length).toBe(before + 1),
        );
    });

    it("should filter branches and switch to a remote branch", async () => {
        mockInvokeHandler("get_agent_git_status", () => repoStatus());
        mockInvokeHandler("list_agent_git_branches", () => [
            branch("main", false, true),
            branch("dev"),
            branch("origin/feature/compact", true),
        ]);
        mockInvokeHandler("switch_agent_git_branch", () => repoStatus({ branch: "feature/compact" }));
        const user = userEvent.setup();
        renderPanel();

        await user.click(await screen.findByText("main"));
        expect(await screen.findByRole("option", { name: "dev" })).toBeInTheDocument();
        expect(screen.getByRole("group", { name: "远程" })).toBeInTheDocument();

        await user.type(screen.getByLabelText("筛选分支…"), "compact");
        expect(screen.queryByRole("option", { name: "dev" })).not.toBeInTheDocument();

        await user.click(screen.getByRole("option", { name: "origin/feature/compact" }));
        expect(invoke).toHaveBeenCalledWith("switch_agent_git_branch", {
            conversationId: 42,
            branch: "origin/feature/compact",
            isRemote: true,
        });
        expect(await screen.findByText("feature/compact")).toBeInTheDocument();
        expect(toastMock.success).toHaveBeenCalledWith("已切换到 feature/compact");
    });

    it("should show the git error when switching branch fails", async () => {
        mockInvokeHandler("get_agent_git_status", () => repoStatus());
        mockInvokeHandler("list_agent_git_branches", () => [branch("main", false, true), branch("dev")]);
        mockInvokeHandler("switch_agent_git_branch", () => {
            throw new Error("git switch dev 失败：local changes would be overwritten");
        });
        const user = userEvent.setup();
        renderPanel();

        await user.click(await screen.findByText("main"));
        await user.click(await screen.findByRole("option", { name: "dev" }));
        await waitFor(() =>
            expect(toastMock.error).toHaveBeenCalledWith("切换分支失败", {
                description: "git switch dev 失败：local changes would be overwritten",
            }),
        );
    });

    it("should let the agent draft the message and commit with push", async () => {
        mockInvokeHandler("get_agent_git_status", () => repoStatus());
        mockInvokeHandler("list_agent_git_changes", () => [
            { path: "src/app.ts", original_path: null, status: "modified", insertions: 3, deletions: 1, binary: false },
        ]);
        mockInvokeHandler("commit_agent_git_changes", () => ({
            commit: "abc1234",
            summary: "feat: generated",
            pushed: true,
            push_error: null,
            status: repoStatus({ has_uncommitted_changes: false }),
        }));
        const user = userEvent.setup();
        const { onGenerateCommitMessage } = renderPanel();

        await user.click(await screen.findByText("提交与推送"));
        expect(await screen.findByText("app.ts")).toBeInTheDocument();
        const commitButton = screen.getByRole("button", { name: "提交" });
        expect(commitButton).toBeDisabled();

        await user.click(screen.getByRole("button", { name: /让 Agent 生成/ }));
        expect(onGenerateCommitMessage).toHaveBeenCalledOnce();
        await waitFor(() => expect(screen.getByLabelText("提交信息")).toHaveValue("feat: generated"));

        await user.click(screen.getByRole("button", { name: "提交并推送" }));
        expect(invoke).toHaveBeenCalledWith("commit_agent_git_changes", {
            conversationId: 42,
            message: "feat: generated",
            push: true,
        });
        await waitFor(() => expect(toastMock.success).toHaveBeenCalledWith("已提交并推送 abc1234", { description: "feat: generated" }));
        expect(await screen.findByText("没有待提交的改动")).toBeInTheDocument();
    });

    it("should report a push failure after a successful commit", async () => {
        mockInvokeHandler("get_agent_git_status", () => repoStatus());
        mockInvokeHandler("list_agent_git_changes", () => []);
        mockInvokeHandler("commit_agent_git_changes", () => ({
            commit: "abc1234",
            summary: "fix",
            pushed: false,
            push_error: "git push 失败：rejected",
            status: repoStatus({ has_uncommitted_changes: false, ahead: 1 }),
        }));
        const user = userEvent.setup();
        renderPanel();

        await user.click(await screen.findByText("提交与推送"));
        await user.type(screen.getByLabelText("提交信息"), "fix");
        await user.click(screen.getByRole("button", { name: "提交并推送" }));
        await waitFor(() =>
            expect(toastMock.error).toHaveBeenCalledWith("已提交 abc1234，但推送失败", { description: "git push 失败：rejected" }),
        );
        expect(await screen.findByText("推送 1 个提交")).toBeInTheDocument();
    });

    it("should push when only local commits are pending", async () => {
        mockInvokeHandler("get_agent_git_status", () => repoStatus({ has_uncommitted_changes: false, ahead: 2 }));
        mockInvokeHandler("push_agent_git_branch", () => repoStatus({ has_uncommitted_changes: false, ahead: 0 }));
        const user = userEvent.setup();
        renderPanel();

        await user.click(await screen.findByText("推送 2 个提交"));
        expect(invoke).toHaveBeenCalledWith("push_agent_git_branch", { conversationId: 42 });
        expect(await screen.findByText("没有待提交的改动")).toBeInTheDocument();
    });

    it("should show the generation failure reason", async () => {
        mockInvokeHandler("get_agent_git_status", () => repoStatus());
        mockInvokeHandler("list_agent_git_changes", () => []);
        const user = userEvent.setup();
        renderPanel({ onGenerateCommitMessage: async () => { throw new Error("会话 42 正在回复，请结束后再试"); } });

        await user.click(await screen.findByText("提交与推送"));
        await act(async () => {
            await user.click(screen.getByRole("button", { name: /让 Agent 生成/ }));
        });
        expect(toastMock.error).toHaveBeenCalledWith("生成提交信息失败", { description: "会话 42 正在回复，请结束后再试" });
    });
});
