import React, { useCallback, useEffect, useState, useMemo } from 'react';
import { invoke } from "@tauri-apps/api/core";
import { Switch } from "../ui/switch";
import { Input } from "../ui/input";
import { Sparkles, RefreshCw, Search } from "lucide-react";
import { toast } from 'sonner';
import { Button } from "../ui/button";
import { Badge } from "../ui/badge";
import { PinyinFilter, usePinyinReady } from "../../utils/pinyinFilter";
import {
    Dialog,
    CompactDialogContent,
    CompactDialogHeader,
    CompactDialogBody,
} from "../ui/dialog";
import {
    ScannedSkill,
    SkillWithConfig,
    groupSkillsBySource,
    SkillSourceType
} from "../../data/Skill";
import ConfirmDialog from "../ConfirmDialog";
import { useSkillsMcpValidation } from "../../hooks/useSkillsMcpValidation";

interface AssistantSkillsConfigDialogProps {
    assistantId: number;
    isOpen: boolean;
    onClose: () => void;
    onConfigChange?: () => void;
}

const AssistantSkillsConfigDialog: React.FC<AssistantSkillsConfigDialogProps> = ({
    assistantId,
    isOpen,
    onClose,
    onConfigChange
}) => {
    const [allSkills, setAllSkills] = useState<ScannedSkill[]>([]);
    const [assistantSkills, setAssistantSkills] = useState<SkillWithConfig[]>([]);
    const [isScanning, setIsScanning] = useState(false);
    const [mcpEnableConfirmOpen, setMcpEnableConfirmOpen] = useState(false);
    const [pendingSkillEnable, setPendingSkillEnable] = useState<{ skill: ScannedSkill; enabled: boolean } | null>(null);
    const [searchQuery, setSearchQuery] = useState('');
    // 有搜索输入时才动态加载 pinyin-pro，加载完成后触发重新过滤
    const pinyinReady = usePinyinReady(searchQuery.trim().length > 0);

    const { checkOperationMcpForSkills, enableOperationMcpAndSkill, isAgentLoadSkillError } = useSkillsMcpValidation();

    // 获取所有可用的 skills 和助手配置
    const fetchSkillsData = useCallback(async () => {
        if (!isOpen) return;

        try {
            setIsScanning(true);

            // 获取所有可用的 skills
            const skills = await invoke<ScannedSkill[]>('scan_skills');
            setAllSkills(skills);

            // 获取助手配置的 skills
            const configs = await invoke<SkillWithConfig[]>('get_assistant_skills', { assistantId });
            setAssistantSkills(configs);

        } catch (error) {
            console.error('Failed to fetch skills data:', error);
            toast.error('获取技能列表失败: ' + error);
        } finally {
            setIsScanning(false);
        }
    }, [assistantId, isOpen]);

    useEffect(() => {
        if (isOpen) {
            fetchSkillsData();
        } else {
            setSearchQuery('');
        }
    }, [isOpen, assistantId]);

    // 检查 skill 是否启用
    const isSkillEnabled = useCallback((identifier: string) => {
        const config = assistantSkills.find(s => s.config.skill_identifier === identifier);
        return config?.config.is_enabled ?? false;
    }, [assistantSkills]);

    // 切换 skill 启用状态
    const handleToggleSkill = useCallback(async (skill: ScannedSkill, enabled: boolean) => {
        // 启用 Skill 前检查 Agent load_skill 状态
        if (enabled) {
            try {
                const checkResult = await checkOperationMcpForSkills(assistantId);
                const hasAgent = !!checkResult.agent_mcp_id;
                if (!hasAgent) {
                    toast.error('未找到智能体工具集（aipp:agent），请先添加或初始化内置工具');
                    return;
                }
                const agentReady = checkResult.agent_ready ??
                    (checkResult.agent_enabled && checkResult.agent_assistant_enabled && checkResult.agent_load_skill_enabled && checkResult.agent_load_skill_assistant_enabled);
                if (!agentReady) {
                    setPendingSkillEnable({ skill, enabled });
                    setMcpEnableConfirmOpen(true);
                    return;
                }
            } catch (error) {
                console.error('Failed to check agent MCP status:', error);
                toast.error('检查智能体工具集状态失败: ' + error);
                return;
            }
        }

        try {
            await invoke('update_assistant_skill_config', {
                assistantId,
                skillIdentifier: skill.identifier,
                isEnabled: enabled,
                priority: 0
            });

            // 刷新配置
            const configs = await invoke<SkillWithConfig[]>('get_assistant_skills', { assistantId });
            setAssistantSkills(configs);

            toast.success(enabled ? '技能已启用' : '技能已禁用');
            onConfigChange?.();
        } catch (error) {
            console.error('Failed to update skill config:', error);
            if (isAgentLoadSkillError(error)) {
                setPendingSkillEnable({ skill, enabled });
                setMcpEnableConfirmOpen(true);
                return;
            }
            toast.error('更新技能配置失败: ' + error);
        }
    }, [assistantId, checkOperationMcpForSkills, isAgentLoadSkillError, onConfigChange]);

    const handleConfirmEnableMcpAndSkill = useCallback(async () => {
        if (!pendingSkillEnable || !pendingSkillEnable.enabled) {
            setMcpEnableConfirmOpen(false);
            setPendingSkillEnable(null);
            return;
        }

        try {
            await enableOperationMcpAndSkill(
                assistantId,
                pendingSkillEnable.skill.identifier,
                0
            );

            const configs = await invoke<SkillWithConfig[]>('get_assistant_skills', { assistantId });
            setAssistantSkills(configs);

            toast.success('已启用技能加载工具并开启技能');
            onConfigChange?.();
        } catch (error) {
            toast.error('启用失败: ' + error);
        } finally {
            setMcpEnableConfirmOpen(false);
            setPendingSkillEnable(null);
        }
    }, [assistantId, enableOperationMcpAndSkill, onConfigChange, pendingSkillEnable]);

    // 按来源分组
    const groupedSkills = useMemo(() => {
        return groupSkillsBySource(allSkills);
    }, [allSkills]);

    // 按搜索词过滤（支持拼音）
    const filteredGroupedSkills = useMemo(() => {
        if (!searchQuery.trim()) return groupedSkills;
        const filtered = new Map<SkillSourceType, ScannedSkill[]>();
        for (const [sourceType, sourceSkills] of groupedSkills.entries()) {
            const matched = sourceSkills.filter(skill =>
                PinyinFilter.matches(skill.display_name, searchQuery) ||
                PinyinFilter.matches(skill.identifier, searchQuery)
            );
            if (matched.length > 0) {
                filtered.set(sourceType, matched);
            }
        }
        return filtered;
    }, [groupedSkills, searchQuery, pinyinReady]);

    // Source order for display
    const sourceOrder: SkillSourceType[] = ['aipp', 'claude_code_agents', 'claude_code_rules', 'claude_code_memory', 'codex'];

    const enabledSkillsCount = assistantSkills.filter(s => s.config.is_enabled && s.exists).length;

    return (
        <>
            <Dialog open={isOpen} onOpenChange={(open) => { if (!open) onClose(); }}>
                <CompactDialogContent className="h-[min(720px,88dvh)] sm:max-w-2xl">
                    <CompactDialogHeader
                        icon={<Sparkles className="h-4 w-4" />}
                        title="技能配置"
                        subtitle={`${enabledSkillsCount} 个已启用`}
                        description={`为该助手配置可用的技能 (${enabledSkillsCount} 个已启用)`}
                        actions={
                            <Button
                                type="button"
                                variant="ghost"
                                size="sm"
                                className="h-8 gap-1 px-2 text-xs"
                                onClick={fetchSkillsData}
                                disabled={isScanning}
                            >
                                <RefreshCw className={`h-3.5 w-3.5 ${isScanning ? 'animate-spin' : ''}`} />
                                刷新
                            </Button>
                        }
                    />

                    <CompactDialogBody className="flex flex-col gap-3 overflow-hidden">
                        {allSkills.length === 0 ? (
                            <div className="flex flex-1 flex-col items-center justify-center py-10 text-center">
                                <Sparkles className="mb-3 h-10 w-10 text-muted-foreground" />
                                <p className="mb-1 text-sm text-muted-foreground">暂无可用的技能</p>
                                <p className="text-xs text-muted-foreground">请先在技能配置中扫描或安装技能</p>
                            </div>
                        ) : (
                            <>
                                {/* 搜索框 */}
                                <div className="relative shrink-0">
                                    <Search className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
                                    <Input
                                        value={searchQuery}
                                        onChange={(e) => setSearchQuery(e.target.value)}
                                        placeholder="搜索技能..."
                                        className="h-8 pl-8 text-sm"
                                    />
                                </div>

                                <div className="min-h-0 flex-1 space-y-4 overflow-y-auto pr-0.5">

                                {sourceOrder.map(sourceType => {
                                    const sourceSkills = filteredGroupedSkills.get(sourceType);
                                    if (!sourceSkills || sourceSkills.length === 0) return null;

                                    return (
                                        <div key={sourceType} className="space-y-2">
                                            <div className="text-xs font-medium text-muted-foreground uppercase tracking-wider px-1">
                                                {sourceSkills[0].source_display_name}
                                            </div>
                                            {sourceSkills.map((skill) => {
                                                const enabled = isSkillEnabled(skill.identifier);

                                                return (
                                                    <div
                                                        key={skill.identifier}
                                                        className={`flex items-center justify-between p-3 rounded-lg border transition-colors ${
                                                            enabled
                                                                ? 'border-border bg-background'
                                                                : 'border-border bg-muted/30'
                                                        }`}
                                                    >
                                                        <div className="flex-1 min-w-0">
                                                            <div className="font-medium text-foreground truncate">
                                                                {skill.display_name}
                                                            </div>
                                                            {skill.metadata.description && (
                                                                <div className="text-sm text-muted-foreground truncate">
                                                                    {skill.metadata.description}
                                                                </div>
                                                            )}
                                                            {skill.metadata.tags.length > 0 && (
                                                                <div className="flex flex-wrap gap-1 mt-1">
                                                                    {skill.metadata.tags.slice(0, 3).map((tag, index) => (
                                                                        <Badge key={index} variant="outline" className="text-xs">
                                                                            {tag}
                                                                        </Badge>
                                                                    ))}
                                                                    {skill.metadata.tags.length > 3 && (
                                                                        <Badge variant="outline" className="text-xs">
                                                                            +{skill.metadata.tags.length - 3}
                                                                        </Badge>
                                                                    )}
                                                                </div>
                                                            )}
                                                        </div>
                                                        <Switch
                                                            checked={enabled}
                                                            onCheckedChange={(checked) => handleToggleSkill(skill, checked)}
                                                            className="ml-3 flex-shrink-0"
                                                        />
                                                    </div>
                                                );
                                            })}
                                        </div>
                                    );
                                })}

                                {/* Custom sources */}
                                {Array.from(filteredGroupedSkills.entries())
                                    .filter(([sourceType]) => !sourceOrder.includes(sourceType))
                                    .map(([sourceType, sourceSkills]) => (
                                        <div key={sourceType} className="space-y-2">
                                            <div className="text-xs font-medium text-muted-foreground uppercase tracking-wider px-1">
                                                {sourceSkills[0].source_display_name}
                                            </div>
                                            {sourceSkills.map((skill) => {
                                                const enabled = isSkillEnabled(skill.identifier);

                                                return (
                                                    <div
                                                        key={skill.identifier}
                                                        className={`flex items-center justify-between p-3 rounded-lg border transition-colors ${
                                                            enabled
                                                                ? 'border-border bg-background'
                                                                : 'border-border bg-muted/30'
                                                        }`}
                                                    >
                                                        <div className="flex-1 min-w-0">
                                                            <div className="font-medium text-foreground truncate">
                                                                {skill.display_name}
                                                            </div>
                                                        </div>
                                                        <Switch
                                                            checked={enabled}
                                                            onCheckedChange={(checked) => handleToggleSkill(skill, checked)}
                                                            className="ml-3 flex-shrink-0"
                                                        />
                                                    </div>
                                                );
                                            })}
                                        </div>
                                    ))}
                                </div>
                            </>
                        )}
                    </CompactDialogBody>
                </CompactDialogContent>
            </Dialog>

            {/* 操作 MCP 启用确认对话框 */}
        <ConfirmDialog
            isOpen={mcpEnableConfirmOpen}
            title="需要启用技能加载工具"
            confirmText="启用技能需要开启智能体工具集的技能加载工具。是否同时启用该工具并启用技能？"
            onConfirm={handleConfirmEnableMcpAndSkill}
            onCancel={() => {
                setMcpEnableConfirmOpen(false);
                setPendingSkillEnable(null);
            }}
            />
        </>
    );
};

export default AssistantSkillsConfigDialog;
