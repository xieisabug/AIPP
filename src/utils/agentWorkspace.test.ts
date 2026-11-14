import { describe, expect, it } from "vitest";

import type { AgentGitBranch } from "@/data/AgentWorkspace";
import {
    agentSupportsManualCompact,
    extractCommitMessage,
    filterBranches,
    formatLineCount,
} from "./agentWorkspace";

const branch = (name: string, isRemote = false): AgentGitBranch => ({
    name,
    full_ref: isRemote ? `refs/remotes/${name}` : `refs/heads/${name}`,
    is_remote: isRemote,
    is_current: false,
    upstream: null,
    committed_at: null,
});

const command = (name: string) => ({ name, description: "" });

describe("agentSupportsManualCompact", () => {
    it("should support Claude Code and Codex without advertised commands", () => {
        expect(agentSupportsManualCompact("claude_sdk")).toBe(true);
        expect(agentSupportsManualCompact("codex_app_server")).toBe(true);
    });

    it("should support ACP agents only when they advertise compact", () => {
        expect(agentSupportsManualCompact("acp", [command("compact")])).toBe(true);
        expect(agentSupportsManualCompact("acp", [command("/compact")])).toBe(true);
        expect(agentSupportsManualCompact("acp", [command("review")])).toBe(false);
        expect(agentSupportsManualCompact("pi")).toBe(false);
        expect(agentSupportsManualCompact(null)).toBe(false);
    });
});

describe("formatLineCount", () => {
    it("should group thousands", () => {
        expect(formatLineCount(1331)).toBe("1,331");
        expect(formatLineCount(37)).toBe("37");
        expect(formatLineCount(-3)).toBe("0");
    });
});

describe("extractCommitMessage", () => {
    it("should take the fenced block when the agent wraps the message", () => {
        expect(extractCommitMessage("好的：\n```\nfeat: add panel\n\n- rows\n```\n")).toBe("feat: add panel\n\n- rows");
    });

    it("should strip a leading label and whitespace", () => {
        expect(extractCommitMessage("  提交信息：fix: race  ")).toBe("fix: race");
        expect(extractCommitMessage("Commit message: chore: bump")).toBe("chore: bump");
    });
});

describe("filterBranches", () => {
    const branches = [branch("main"), branch("feature/plan-toggle"), branch("origin/feature/compact", true)];

    it("should return all branches for an empty query", () => {
        expect(filterBranches(branches, "  ")).toHaveLength(3);
    });

    it("should match every term case-insensitively", () => {
        expect(filterBranches(branches, "FEATURE").map((b) => b.name)).toEqual(["feature/plan-toggle", "origin/feature/compact"]);
        expect(filterBranches(branches, "origin compact").map((b) => b.name)).toEqual(["origin/feature/compact"]);
    });
});
