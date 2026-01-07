import React, { useCallback, useEffect, useState, useMemo } from 'react';
import { invoke } from "@tauri-apps/api/core";
import { Switch } from "../ui/switch";
import { Sparkles, RefreshCw } from "lucide-react";
import { toast } from 'sonner';
import { Button } from "../ui/button";
import { Badge } from "../ui/badge";
import {
    Dialog,
    DialogContent,
    DialogDescription,
    DialogHeader,
    DialogTitle,
} from "../ui/dialog";
import {
    ScannedSkill,
    SkillWithConfig,
    getSourceDisplayName,
    groupSkillsBySource,
    SkillSourceType
} from "../../data/Skill";

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
            toast.error('获取Skills列表失败: ' + error);
        } finally {
            setIsScanning(false);
        }
    }, [assistantId, isOpen]);

    useEffect(() => {
        fetchSkillsData();
    }, [fetchSkillsData]);

    // 检查 skill 是否启用
    const isSkillEnabled = useCallback((identifier: string) => {
        const config = assistantSkills.find(s => s.config.skill_identifier === identifier);
        return config?.config.is_enabled ?? false;
    }, [assistantSkills]);

    // 切换 skill 启用状态
    const handleToggleSkill = useCallback(async (skill: ScannedSkill, enabled: boolean) => {
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

            toast.success(enabled ? 'Skill已启用' : 'Skill已禁用');
            onConfigChange?.();
        } catch (error) {
            console.error('Failed to update skill config:', error);
            toast.error('更新Skill配置失败: ' + error);
        }
    }, [assistantId, onConfigChange]);

    // 按来源分组
    const groupedSkills = useMemo(() => {
        return groupSkillsBySource(allSkills);
    }, [allSkills]);

    // Source order for display
    const sourceOrder: SkillSourceType[] = ['aipp', 'claude_code_agents', 'claude_code_rules', 'claude_code_memory', 'codex'];

    const enabledSkillsCount = assistantSkills.filter(s => s.config.is_enabled && s.exists).length;

    return (
        <Dialog open={isOpen} onOpenChange={onClose}>
            <DialogContent className="max-w-2xl max-h-[80vh] overflow-hidden flex flex-col">
                <DialogHeader>
                    <div className="flex items-center justify-between">
                        <div>
                            <DialogTitle className="flex items-center gap-2">
                                <Sparkles className="h-5 w-5" />
                                Skills配置
                            </DialogTitle>
                            <DialogDescription>
                                为该助手配置可用的Skills ({enabledSkillsCount}个已启用)
                            </DialogDescription>
                        </div>
                        <Button
                            variant="outline"
                            size="sm"
                            onClick={fetchSkillsData}
                            disabled={isScanning}
                        >
                            <RefreshCw className={`h-4 w-4 mr-1 ${isScanning ? 'animate-spin' : ''}`} />
                            刷新
                        </Button>
                    </div>
                </DialogHeader>

                <div className="flex-1 overflow-auto">
                    {allSkills.length === 0 ? (
                        <div className="text-center py-8">
                            <Sparkles className="h-12 w-12 text-muted-foreground mx-auto mb-4" />
                            <p className="text-sm text-muted-foreground mb-2">暂无可用的Skills</p>
                            <p className="text-xs text-muted-foreground">请先在Skills配置中扫描或安装Skills</p>
                        </div>
                    ) : (
                        <div className="space-y-4">
                            {sourceOrder.map(sourceType => {
                                const sourceSkills = groupedSkills.get(sourceType);
                                if (!sourceSkills || sourceSkills.length === 0) return null;

                                return (
                                    <div key={sourceType} className="space-y-2">
                                        <div className="text-xs font-medium text-muted-foreground uppercase tracking-wider px-1">
                                            {getSourceDisplayName(sourceType)}
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
                            {Array.from(groupedSkills.entries())
                                .filter(([sourceType]) => !sourceOrder.includes(sourceType))
                                .map(([sourceType, sourceSkills]) => (
                                    <div key={sourceType} className="space-y-2">
                                        <div className="text-xs font-medium text-muted-foreground uppercase tracking-wider px-1">
                                            {getSourceDisplayName(sourceType)}
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
                    )}
                </div>
            </DialogContent>
        </Dialog>
    );
};

export default AssistantSkillsConfigDialog;
