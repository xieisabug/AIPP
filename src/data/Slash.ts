export interface SlashNamespaceItem {
    name: string;
    description: string;
    isEnabled: boolean;
}

export interface SlashSkillCompletionItem {
    identifier: string;
    displayName: string;
    invokeName: string;
    aliases: string[];
    sourceType: string;
    sourceDisplayName: string;
    description?: string | null;
    tags: string[];
}

export interface FilteredSlashSkill extends SlashSkillCompletionItem {
    matchType: "exact" | "pinyin" | "initial";
    highlightIndices: number[];
}

export const DEFAULT_SLASH_NAMESPACES: SlashNamespaceItem[] = [
    {
        name: "skills",
        description: "主动调用已安装 Skills",
        isEnabled: true,
    },
    {
        name: "artifacts",
        description: "选择/引用 Artifact（后续）",
        isEnabled: false,
    },
];
