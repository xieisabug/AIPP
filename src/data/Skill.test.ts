import { describe, expect, it } from "vitest";
import { orderedSkillGroups, type ScannedSkill } from "./Skill";

const skill = (source_type: string, source_display_name: string, name: string): ScannedSkill => ({
    identifier: `${source_type}:${name}`,
    source_type,
    source_display_name,
    file_path: `C:/skills/${name}/SKILL.md`,
    relative_path: name,
    display_name: name,
    metadata: { name, description: null, version: null, author: null, tags: [], requires_files: [] },
    exists: true,
});

describe("orderedSkillGroups", () => {
    it("should order built-in sources first and keep other sources in scan order", () => {
        const groups = orderedSkillGroups([
            skill("agent_plugin", "插件 · GitHub", "review-pr"),
            skill("codex", "Codex", "imagegen"),
            skill("my_source", "我的目录", "notes"),
            skill("claude_code_skills", "Claude Code Skills", "pdf"),
            skill("agents", "Agents", "brainstorming"),
        ]);
        expect(groups.map((group) => group.label)).toEqual([
            "Agents", "Claude Code Skills", "Codex", "插件 · GitHub", "我的目录",
        ]);
    });

    it("should give each plugin bundle its own group when plugin skills share a source type", () => {
        const groups = orderedSkillGroups([
            skill("agent_plugin", "插件 · GitHub", "review-pr"),
            skill("agent_plugin", "插件 · Linear", "linear-sync"),
            skill("agent_plugin", "插件 · GitHub", "triage"),
        ]);
        expect(groups.map((group) => [group.label, group.skills.map((s) => s.display_name)])).toEqual([
            ["插件 · GitHub", ["review-pr", "triage"]],
            ["插件 · Linear", ["linear-sync"]],
        ]);
    });
});
