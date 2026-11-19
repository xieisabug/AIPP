// Skill types - matches Rust types in src-tauri/src/skills/types.rs

/** Skill source type - identifies where the skill comes from */
export type SkillSourceType =
  | 'agents'
  | 'claude_code_skills'
  | 'codex'
  | 'copilot'
  | 'agent_plugin' // GitHub 插件包里的技能，source_display_name 为「插件 · 包名」
  | string; // Custom sources

/** Configuration for a skill source */
export interface SkillSourceConfig {
  source_type: SkillSourceType;
  display_name: string;
  paths: string[];
  file_pattern: string;
  is_enabled: boolean;
  is_builtin: boolean;
}

/** Metadata extracted from SKILL.md frontmatter */
export interface SkillMetadata {
  name: string | null;
  description: string | null;
  version: string | null;
  author: string | null;
  tags: string[];
  requires_files: string[];
}

/** A scanned skill with metadata */
export interface ScannedSkill {
  /** Unique identifier: "{source_type}:{relative_path}" */
  identifier: string;
  /** Source type */
  source_type: SkillSourceType;
  /** Display name for the source type (from backend) */
  source_display_name: string;
  /** Absolute path to the skill file */
  file_path: string;
  /** Relative path within the source */
  relative_path: string;
  /** Extracted metadata from frontmatter */
  metadata: SkillMetadata;
  /** Display name (from metadata.name or filename) */
  display_name: string;
  /** Whether the skill file exists */
  exists: boolean;
}

/** Skill content - full content loaded on demand */
export interface SkillContent {
  identifier: string;
  content: string;
  additional_files: SkillFile[];
}

/** Additional file content for a skill */
export interface SkillFile {
  path: string;
  content: string;
}

/** Assistant's skill configuration */
export interface AssistantSkillConfig {
  id: number;
  assistant_id: number;
  skill_identifier: string;
  is_enabled: boolean;
  priority: number;
  created_time: string;
}

/** Skill with config and existence status */
export interface SkillWithConfig {
  skill: ScannedSkill | null;
  config: AssistantSkillConfig;
  exists: boolean;
}

/** Parse skill identifier to extract source type and relative path */
export function parseSkillIdentifier(identifier: string): { sourceType: SkillSourceType; relativePath: string } | null {
  const colonIndex = identifier.indexOf(':');
  if (colonIndex === -1) return null;
  
  return {
    sourceType: identifier.substring(0, colonIndex) as SkillSourceType,
    relativePath: identifier.substring(colonIndex + 1),
  };
}

/** Group skills by source type */
export function groupSkillsBySource(skills: ScannedSkill[]): Map<SkillSourceType, ScannedSkill[]> {
  const groups = new Map<SkillSourceType, ScannedSkill[]>();
  
  for (const skill of skills) {
    const existing = groups.get(skill.source_type) || [];
    existing.push(skill);
    groups.set(skill.source_type, existing);
  }
  
  return groups;
}

/** 一组来源相同的 Skills（设置页列表的一个分组）。 */
export interface SkillGroup {
  key: string;
  label: string;
  sourceType: SkillSourceType;
  skills: ScannedSkill[];
}

/** 内置来源的显示顺序；其他来源（插件包、自定义）按扫描顺序排在后面。 */
const SKILL_SOURCE_ORDER: SkillSourceType[] = ['agents', 'claude_code_skills', 'codex', 'copilot'];

/**
 * 按来源分组并排序。插件包技能的来源类型都是 agent_plugin，
 * 按显示名（「插件 · 包名」）再分开，每个插件包一组。
 */
export function orderedSkillGroups(skills: ScannedSkill[]): SkillGroup[] {
  const groups = new Map<string, SkillGroup>();
  for (const skill of skills) {
    const key = `${skill.source_type}:${skill.source_display_name}`;
    const group = groups.get(key);
    if (group) group.skills.push(skill);
    else groups.set(key, { key, label: skill.source_display_name, sourceType: skill.source_type, skills: [skill] });
  }
  const rank = (group: SkillGroup) => {
    const index = SKILL_SOURCE_ORDER.indexOf(group.sourceType);
    return index === -1 ? SKILL_SOURCE_ORDER.length : index;
  };
  return Array.from(groups.values()).sort((a, b) => rank(a) - rank(b));
}
