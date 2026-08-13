import { describe, expect, it } from "vitest";

import PinyinFilter, { type AssistantItem } from "./pinyinFilter";
import type { ArtifactCollectionItem } from "../data/ArtifactCollection";
import type { SlashSkillCompletionItem } from "../data/Slash";

function makeArtifact(name: string): ArtifactCollectionItem {
    return {
        id: 1,
        name,
        icon: "",
        description: "",
        artifact_type: "html",
        created_time: "",
        use_count: 0,
    };
}

function makeAssistant(name: string): AssistantItem {
    return {
        id: 1,
        name,
    };
}

function makeSlashSkill(displayName: string, invokeName: string): SlashSkillCompletionItem {
    return {
        identifier: `skill:${invokeName}`,
        displayName,
        invokeName,
        aliases: [],
        sourceType: "local",
        sourceDisplayName: "Local",
        description: null,
        tags: [],
    };
}

describe("PinyinFilter", () => {
    it("filters artifacts by cached full pinyin", () => {
        const result = PinyinFilter.filterArtifacts(
            [makeArtifact("识图测试")],
            "shitu",
        );

        expect(result).toHaveLength(1);
        expect(result[0].matchType).toBe("pinyin");
        expect(result[0].highlightIndices).toEqual([0, 1]);
    });

    it("filters assistants by cached initials", () => {
        const result = PinyinFilter.filterAssistants(
            [makeAssistant("市场监管助手")],
            "scjg",
        );

        expect(result).toHaveLength(1);
        expect(result[0].matchType).toBe("initial");
        expect(result[0].highlightIndices).toEqual([0, 1, 2, 3]);
    });

    it("filters slash skills by display name pinyin", () => {
        const result = PinyinFilter.filterSlashSkills(
            [makeSlashSkill("业务学习", "study")],
            "yewu",
        );

        expect(result).toHaveLength(1);
        expect(result[0].matchType).toBe("pinyin");
        expect(result[0].highlightIndices).toEqual([0, 1]);
    });

    it("matches text by pinyin without requiring highlighted results", () => {
        expect(PinyinFilter.matches("模型配置", "mxpz")).toBe(true);
        expect(PinyinFilter.matches("模型配置", "unknown")).toBe(false);
    });
});
