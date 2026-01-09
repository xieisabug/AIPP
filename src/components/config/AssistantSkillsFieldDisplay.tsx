import React, { useCallback, useEffect, useState } from 'react';
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Button } from "../ui/button";
import { Settings, Sparkles } from "lucide-react";
import AssistantSkillsConfigDialog from './AssistantSkillsConfigDialog';

interface AssistantSkillsFieldDisplayProps {
    assistantId: number;
    onConfigChange?: () => void;
    navigateTo: (menuKey: string) => void;
}

interface SkillsSummary {
    totalSkills: number;
    enabledSkills: number;
}

const AssistantSkillsFieldDisplay: React.FC<AssistantSkillsFieldDisplayProps> = ({
    assistantId,
    onConfigChange,
    navigateTo
}) => {
    const [skillsSummary, setSkillsSummary] = useState<SkillsSummary>({
        totalSkills: 0,
        enabledSkills: 0
    });
    const [configDialogOpen, setConfigDialogOpen] = useState(false);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);

    // 获取 Skills 配置摘要
    const fetchSkillsSummary = useCallback(async () => {
        try {
            setLoading(true);
            setError(null);

            // 获取所有可用的 skills
            const allSkills = await invoke<any[]>('scan_skills');
            
            // 获取助手配置的 skills
            const assistantSkills = await invoke<any[]>('get_assistant_skills', { assistantId });

            const totalSkills = allSkills.length;
            const enabledSkills = assistantSkills.filter(s => s.config.is_enabled && s.exists).length;

            setSkillsSummary({
                totalSkills,
                enabledSkills
            });

        } catch (error) {
            console.error('Failed to fetch skills summary:', error);
            setError(error as string);
        } finally {
            setLoading(false);
        }
    }, [assistantId]);

    useEffect(() => {
        fetchSkillsSummary();

        // 监听 MCP 状态变更事件，Skills 可能被自动禁用
        const unlistenPromise = listen('mcp_state_changed', () => {
            fetchSkillsSummary();
        });

        return () => {
            unlistenPromise.then(unlisten => unlisten());
        };
    }, [fetchSkillsSummary]);

    const handleOpenConfig = useCallback(() => {
        if (skillsSummary.totalSkills === 0) {
            // 跳转到 skills 的配置页面
            navigateTo("skills-config");
        } else {
            setConfigDialogOpen(true);
        }
    }, [skillsSummary.totalSkills, navigateTo]);

    const handleCloseConfig = useCallback(() => {
        setConfigDialogOpen(false);
    }, []);

    const handleConfigChanged = useCallback(() => {
        fetchSkillsSummary(); // 刷新摘要数据
        onConfigChange?.(); // 通知父组件配置已更改
    }, [fetchSkillsSummary, onConfigChange]);

    const getSummaryText = () => {
        if (loading) {
            return "加载中...";
        }

        if (error) {
            return "加载失败";
        }

        if (skillsSummary.totalSkills === 0) {
            return "暂无可用的Skills";
        }

        return `已启用 ${skillsSummary.enabledSkills} 个Skill`;
    };

    return (
        <>
            <div className="flex items-center justify-between">
                <div className="flex items-start gap-3">
                    <div>
                        <div className="text-sm font-medium text-foreground">{getSummaryText()}</div>
                    </div>
                </div>

                <Button
                    variant={skillsSummary.totalSkills === 0 ? "default" : "outline"}
                    size="sm"
                    onClick={handleOpenConfig}
                    disabled={loading}
                >
                    {skillsSummary.totalSkills === 0 ? (
                        <>
                            <Sparkles className="h-4 w-4 mr-1" />
                            扫描Skills
                        </>
                    ) : (
                        <>
                            <Settings className="h-4 w-4 mr-1" />
                            配置
                        </>
                    )}
                </Button>
            </div>

            <AssistantSkillsConfigDialog
                assistantId={assistantId}
                isOpen={configDialogOpen}
                onClose={handleCloseConfig}
                onConfigChange={handleConfigChanged}
            />
        </>
    );
};

export default AssistantSkillsFieldDisplay;
