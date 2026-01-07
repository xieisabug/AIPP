import React, { useCallback, useEffect, useState, useMemo } from 'react';
import { invoke } from "@tauri-apps/api/core";
import { Button } from "../ui/button";
import { Switch } from "../ui/switch";
import { Badge } from "../ui/badge";
import { Folder, Sparkles, RefreshCw, FolderOpen, FileText } from "lucide-react";
import { Tooltip, TooltipTrigger, TooltipContent } from "../ui/tooltip";
import { toast } from 'sonner';

import {
    ConfigPageLayout,
    SidebarList,
    ListItemButton,
    EmptyState,
    SelectOption
} from "../common";

import {
    ScannedSkill,
    SkillWithConfig,
    getSourceDisplayName,
    groupSkillsBySource,
    SkillSourceType
} from "../../data/Skill";

import SkillActionDropdown from "./SkillActionDropdown";

interface SkillsManagerProps {
    /** Optional: if provided, shows skills config for an assistant */
    assistantId?: number;
}

const SkillsManager: React.FC<SkillsManagerProps> = ({ assistantId }) => {
    const [skills, setSkills] = useState<ScannedSkill[]>([]);
    const [selectedSkill, setSelectedSkill] = useState<ScannedSkill | null>(null);
    const [skillContent, setSkillContent] = useState<string>('');
    const [isLoading, setIsLoading] = useState(false);
    const [isRefreshing, setIsRefreshing] = useState(false);

    // Assistant skill configs (only used when assistantId is provided)
    const [assistantSkills, setAssistantSkills] = useState<SkillWithConfig[]>([]);

    // Scan all skills
    const scanSkills = useCallback(async () => {
        setIsRefreshing(true);
        try {
            const scannedSkills = await invoke<ScannedSkill[]>('scan_skills');
            setSkills(scannedSkills);

            // If no skill selected and we have skills, select first one
            if (!selectedSkill && scannedSkills.length > 0) {
                setSelectedSkill(scannedSkills[0]);
            }
        } catch (e) {
            toast.error('扫描Skills失败: ' + e);
        } finally {
            setIsRefreshing(false);
        }
    }, [selectedSkill]);

    // Load assistant skill configs
    const loadAssistantSkills = useCallback(async () => {
        if (!assistantId) return;

        try {
            const configs = await invoke<SkillWithConfig[]>('get_assistant_skills', { assistantId });
            setAssistantSkills(configs);
        } catch (e) {
            toast.error('获取助手Skills配置失败: ' + e);
        }
    }, [assistantId]);

    // Load skill content
    const loadSkillContent = useCallback(async (identifier: string) => {
        setIsLoading(true);
        try {
            const content = await invoke<{ content: string }>('get_skill_content', { identifier });
            setSkillContent(content.content);
        } catch (e) {
            toast.error('加载Skill内容失败: ' + e);
            setSkillContent('');
        } finally {
            setIsLoading(false);
        }
    }, []);

    useEffect(() => {
        scanSkills();
    }, []);

    useEffect(() => {
        if (assistantId) {
            loadAssistantSkills();
        }
    }, [assistantId, loadAssistantSkills]);

    useEffect(() => {
        if (selectedSkill) {
            loadSkillContent(selectedSkill.identifier);
        }
    }, [selectedSkill, loadSkillContent]);

    // Handle skill selection
    const handleSelectSkill = useCallback((skill: ScannedSkill) => {
        setSelectedSkill(skill);
    }, []);

    // Toggle skill for assistant
    const handleToggleSkill = useCallback(async (skill: ScannedSkill, enabled: boolean) => {
        if (!assistantId) return;

        try {
            await invoke('update_assistant_skill_config', {
                assistantId,
                skillIdentifier: skill.identifier,
                isEnabled: enabled,
                priority: 0
            });

            // Refresh configs
            await loadAssistantSkills();

            toast.success(enabled ? 'Skill已启用' : 'Skill已禁用');
        } catch (e) {
            toast.error('更新Skill配置失败: ' + e);
        }
    }, [assistantId, loadAssistantSkills]);

    // Open skills folder
    const handleOpenSkillsFolder = useCallback(async () => {
        try {
            await invoke('open_skills_folder');
        } catch (e) {
            toast.error('打开文件夹失败: ' + e);
        }
    }, []);

    // Open skill's parent folder
    const handleOpenSkillFolder = useCallback(async (filePath: string) => {
        try {
            await invoke('open_skill_parent_folder', { filePath });
        } catch (e) {
            toast.error('打开文件夹失败: ' + e);
        }
    }, []);

    // Install official skills
    const handleInstallOfficial = useCallback(() => {
        // Open AIPP official skills repository
        window.open('https://github.com/aipp-org/skills', '_blank');
    }, []);

    // Group skills by source
    const groupedSkills = useMemo(() => {
        return groupSkillsBySource(skills);
    }, [skills]);

    // Source order for display
    const sourceOrder: SkillSourceType[] = ['aipp', 'claude_code_agents', 'claude_code_rules', 'claude_code_memory', 'codex'];

    // Check if skill is enabled for assistant
    const isSkillEnabled = useCallback((identifier: string) => {
        const config = assistantSkills.find(s => s.config.skill_identifier === identifier);
        return config?.config.is_enabled ?? false;
    }, [assistantSkills]);

    // Get config for skill
    const getSkillConfig = useCallback((identifier: string) => {
        return assistantSkills.find(s => s.config.skill_identifier === identifier);
    }, [assistantSkills]);

    // Select options for mobile dropdown
    const selectOptions: SelectOption[] = useMemo(() =>
        skills.map(skill => ({
            id: skill.identifier,
            label: skill.display_name,
            icon: <Sparkles className="h-4 w-4" />
        })), [skills]);

    // Handle select from dropdown
    const handleSelectFromDropdown = useCallback((identifier: string) => {
        const skill = skills.find(s => s.identifier === identifier);
        if (skill) {
            handleSelectSkill(skill);
        }
    }, [skills, handleSelectSkill]);

    // Sidebar content
    const sidebar = useMemo(() => (
        <SidebarList
            title="Skills"
            description="AI可用的技能和指令"
            icon={<Sparkles className="h-5 w-5" />}
            addButton={
                <SkillActionDropdown
                    onScan={scanSkills}
                    onOpenFolder={handleOpenSkillsFolder}
                    onInstallOfficial={handleInstallOfficial}
                    isScanning={isRefreshing}
                    showIcon={false}
                    variant="outline"
                    size="sm"
                />
            }
        >
            {sourceOrder.map(sourceType => {
                const sourceSkills = groupedSkills.get(sourceType);
                if (!sourceSkills || sourceSkills.length === 0) return null;

                return (
                    <div key={sourceType} className="mb-4">
                        <div className="text-xs font-medium text-muted-foreground px-2 py-1 uppercase tracking-wider">
                            {getSourceDisplayName(sourceType)}
                        </div>
                        {sourceSkills.map((skill) => {
                            const config = getSkillConfig(skill.identifier);
                            const hasConfig = !!config;
                            const isEnabled = config?.config.is_enabled ?? false;

                            return (
                                <ListItemButton
                                    key={skill.identifier}
                                    isSelected={selectedSkill?.identifier === skill.identifier}
                                    onClick={() => handleSelectSkill(skill)}
                                >
                                    <div className="flex items-center w-full">
                                        <span className="flex-1 truncate font-medium">{skill.display_name}</span>
                                        {assistantId && hasConfig && isEnabled && (
                                            <Badge variant="secondary" className="ml-2 flex-shrink-0 text-xs">
                                                启用
                                            </Badge>
                                        )}
                                    </div>
                                </ListItemButton>
                            );
                        })}
                    </div>
                );
            })}

            {/* Custom sources */}
            {Array.from(groupedSkills.entries())
                .filter(([sourceType]) => !sourceOrder.includes(sourceType))
                .map(([sourceType, sourceSkills]) => (
                    <div key={sourceType} className="mb-4">
                        <div className="text-xs font-medium text-muted-foreground px-2 py-1 uppercase tracking-wider">
                            {getSourceDisplayName(sourceType)}
                        </div>
                        {sourceSkills.map((skill) => (
                            <ListItemButton
                                key={skill.identifier}
                                isSelected={selectedSkill?.identifier === skill.identifier}
                                onClick={() => handleSelectSkill(skill)}
                            >
                                <span className="flex-1 truncate font-medium">{skill.display_name}</span>
                            </ListItemButton>
                        ))}
                    </div>
                ))}
        </SidebarList>
    ), [groupedSkills, selectedSkill, isRefreshing, assistantId, getSkillConfig, handleSelectSkill, scanSkills, handleOpenSkillsFolder, handleInstallOfficial]);

    // Main content
    const content = useMemo(() => selectedSkill ? (
        <div className="space-y-6">
            {/* Skill header */}
            <div className="bg-background rounded-lg border border-border p-6">
                <div className="flex items-center justify-between mb-4">
                    <div className="flex-1">
                        <h3 className="text-lg font-semibold text-foreground">{selectedSkill.display_name}</h3>
                        {selectedSkill.metadata.description && (
                            <p className="text-sm text-muted-foreground mt-1">{selectedSkill.metadata.description}</p>
                        )}
                    </div>
                    {assistantId && (
                        <div className="flex items-center gap-2">
                            <span className="text-sm text-muted-foreground">为此助手启用</span>
                            <Switch
                                checked={isSkillEnabled(selectedSkill.identifier)}
                                onCheckedChange={(checked) => handleToggleSkill(selectedSkill, checked)}
                            />
                        </div>
                    )}
                </div>

                <div className="flex flex-wrap gap-4 text-sm">
                    <div>
                        <span className="font-medium text-foreground">来源:</span>
                        <Badge variant="secondary" className="ml-2">
                            {getSourceDisplayName(selectedSkill.source_type)}
                        </Badge>
                    </div>
                    {selectedSkill.metadata.version && (
                        <div>
                            <span className="font-medium text-foreground">版本:</span>
                            <span className="ml-2 text-muted-foreground">{selectedSkill.metadata.version}</span>
                        </div>
                    )}
                    {selectedSkill.metadata.author && (
                        <div>
                            <span className="font-medium text-foreground">作者:</span>
                            <span className="ml-2 text-muted-foreground">{selectedSkill.metadata.author}</span>
                        </div>
                    )}
                </div>

                {selectedSkill.metadata.tags.length > 0 && (
                    <div className="flex flex-wrap gap-1 mt-3">
                        {selectedSkill.metadata.tags.map((tag, index) => (
                            <Badge key={index} variant="outline" className="text-xs">
                                {tag}
                            </Badge>
                        ))}
                    </div>
                )}

                <div className="mt-3 flex items-center gap-2 text-xs text-muted-foreground">
                    <span className="font-medium">路径:</span>
                    <span className="flex-1 truncate">{selectedSkill.file_path}</span>
                    <Tooltip delayDuration={300}>
                        <TooltipTrigger asChild>
                            <Button
                                variant="ghost"
                                size="icon"
                                className="h-6 w-6 flex-shrink-0"
                                onClick={() => handleOpenSkillFolder(selectedSkill.file_path)}
                            >
                                <FolderOpen className="h-4 w-4" />
                            </Button>
                        </TooltipTrigger>
                        <TooltipContent>打开所在文件夹</TooltipContent>
                    </Tooltip>
                </div>
            </div>

            {/* Skill content preview */}
            <div className="bg-background rounded-lg border border-border p-6">
                <h4 className="text-md font-semibold text-foreground mb-4">内容预览</h4>
                {isLoading ? (
                    <div className="flex items-center justify-center py-8">
                        <RefreshCw className="h-6 w-6 animate-spin text-muted-foreground" />
                    </div>
                ) : (
                    <div className="bg-muted rounded-lg p-4 max-h-96 overflow-auto">
                        <pre className="text-sm text-foreground whitespace-pre-wrap font-mono">
                            {skillContent || '(无内容)'}
                        </pre>
                    </div>
                )}
            </div>

            {/* Required files */}
            {selectedSkill.metadata.requires_files.length > 0 && (
                <div className="bg-background rounded-lg border border-border p-6">
                    <h4 className="text-md font-semibold text-foreground mb-4">关联文件</h4>
                    <div className="space-y-2">
                        {selectedSkill.metadata.requires_files.map((file, index) => (
                            <div key={index} className="flex items-center gap-2 text-sm text-muted-foreground">
                                <FileText className="h-4 w-4" />
                                <span>{file}</span>
                            </div>
                        ))}
                    </div>
                </div>
            )}
        </div>
    ) : (
        <EmptyState
            icon={<Sparkles className="h-8 w-8 text-muted-foreground" />}
            title="选择一个Skill"
            description="从左侧列表中选择一个Skill查看详情"
        />
    ), [selectedSkill, skillContent, isLoading, assistantId, isSkillEnabled, handleToggleSkill, handleOpenSkillFolder]);

    // Empty state when no skills found
    if (skills.length === 0 && !isRefreshing) {
        return (
            <ConfigPageLayout
                sidebar={null}
                content={null}
                emptyState={
                    <EmptyState
                        icon={<Sparkles className="h-8 w-8 text-muted-foreground" />}
                        title="暂无Skills"
                        description="点击扫描按钮自动发现已安装的Skills，将扫描以下目录：~/.claude/agents、~/.claude/rules、~/.codex/skills/.system 以及应用数据目录"
                        action={
                            <div className="flex gap-2">
                                <Button onClick={scanSkills} disabled={isRefreshing}>
                                    <RefreshCw className={`h-4 w-4 mr-2 ${isRefreshing ? 'animate-spin' : ''}`} />
                                    扫描Skills
                                </Button>
                                <Button variant="outline" onClick={handleOpenSkillsFolder}>
                                    <Folder className="h-4 w-4 mr-2" />
                                    打开Skills文件夹
                                </Button>
                            </div>
                        }
                    />
                }
                showEmptyState={true}
            />
        );
    }

    return (
        <ConfigPageLayout
            sidebar={sidebar}
            content={content}
            selectOptions={selectOptions}
            selectedOptionId={selectedSkill?.identifier}
            onSelectOption={handleSelectFromDropdown}
            selectPlaceholder="选择Skill"
        />
    );
};

export default SkillsManager;
