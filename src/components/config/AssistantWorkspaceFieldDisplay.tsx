import React, { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "../ui/button";
import { FolderOpen, Plus, Trash2 } from "lucide-react";
import {
    Dialog,
    CompactDialogContent,
    CompactDialogHeader,
    CompactDialogBody,
    DialogTrigger,
} from "../ui/dialog";
import { AssistantWorkspace } from "@/data/Assistant";
import { FolderPicker } from "./FolderPicker";

interface AssistantWorkspaceFieldDisplayProps {
    assistantId: number;
    onConfigChange?: () => void;
}

const AssistantWorkspaceFieldDisplay: React.FC<AssistantWorkspaceFieldDisplayProps> = ({
    assistantId,
    onConfigChange,
}) => {
    const [workspaces, setWorkspaces] = useState<AssistantWorkspace[]>([]);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);
    const [dialogOpen, setDialogOpen] = useState(false);
    const [newPath, setNewPath] = useState("");

    const fetchWorkspaces = useCallback(async () => {
        try {
            setLoading(true);
            setError(null);
            const result = await invoke<AssistantWorkspace[]>("get_assistant_workspaces", {
                assistantId,
            });
            setWorkspaces(result);
        } catch (err) {
            console.error("Failed to fetch workspaces:", err);
            setError(err as string);
        } finally {
            setLoading(false);
        }
    }, [assistantId]);

    useEffect(() => {
        fetchWorkspaces();
    }, [fetchWorkspaces]);

    const handleAddWorkspace = useCallback(async () => {
        if (!newPath.trim()) return;

        try {
            await invoke("add_assistant_workspace", {
                assistantId,
                path: newPath.trim(),
            });
            setNewPath("");
            fetchWorkspaces();
            onConfigChange?.();
        } catch (err) {
            console.error("Failed to add workspace:", err);
            setError(err as string);
        }
    }, [assistantId, newPath, fetchWorkspaces, onConfigChange]);

    const handleRemoveWorkspace = useCallback(
        async (workspaceId: number) => {
            try {
                await invoke("remove_assistant_workspace", {
                    id: workspaceId,
                });
                fetchWorkspaces();
                onConfigChange?.();
            } catch (err) {
                console.error("Failed to remove workspace:", err);
                setError(err as string);
            }
        },
        [fetchWorkspaces, onConfigChange]
    );

    const getSummaryText = () => {
        if (loading) {
            return "加载中...";
        }

        if (error) {
            return "加载失败";
        }

        if (workspaces.length === 0) {
            return "暂无信任路径";
        }

        return `已配置 ${workspaces.length} 个信任路径`;
    };

    return (
        <div className="space-y-2">
            <div className="flex items-center justify-between">
                <div className="text-sm text-muted-foreground">{getSummaryText()}</div>
                <Dialog open={dialogOpen} onOpenChange={setDialogOpen}>
                    <DialogTrigger asChild>
                        <Button variant="outline" size="sm" disabled={loading}>
                            <FolderOpen className="h-4 w-4 mr-1" />
                            管理
                        </Button>
                    </DialogTrigger>
                    <CompactDialogContent className="sm:max-w-lg">
                        <CompactDialogHeader
                            icon={<FolderOpen className="h-4 w-4" />}
                            title="助手工作区配置"
                            subtitle={workspaces.length > 0 ? `${workspaces.length} 个路径` : undefined}
                            description="配置助手信任的文件路径，路径下的文件操作将自动放行"
                        />
                        <CompactDialogBody className="space-y-4">
                            <p className="text-xs text-muted-foreground">
                                配置助手信任的文件路径。在此路径下的文件操作将自动放行，无需每次确认。
                            </p>

                            {/* 添加新路径 */}
                            <div className="flex items-center gap-2">
                                <FolderPicker
                                    value={newPath}
                                    onChange={setNewPath}
                                    placeholder="选择或输入路径"
                                />
                                <Button
                                    size="sm"
                                    onClick={handleAddWorkspace}
                                    disabled={!newPath.trim()}
                                >
                                    <Plus className="mr-1 h-4 w-4" />
                                    添加
                                </Button>
                            </div>

                            {/* 现有路径列表 */}
                            <div className="max-h-64 space-y-1.5 overflow-y-auto pr-0.5">
                                {workspaces.length === 0 ? (
                                    <div className="rounded-lg border border-dashed border-border bg-muted/20 py-6 text-center text-sm text-muted-foreground">
                                        暂无信任路径
                                    </div>
                                ) : (
                                    workspaces.map((workspace) => (
                                        <div
                                            key={workspace.id}
                                            className="flex items-center justify-between gap-2 rounded-lg border border-border bg-muted/40 px-3 py-2"
                                        >
                                            <span className="flex-1 break-all font-mono text-xs">
                                                {workspace.path}
                                            </span>
                                            <Button
                                                iconLabel="移除工作区"
                                                variant="ghost"
                                                size="icon"
                                                onClick={() =>
                                                    handleRemoveWorkspace(workspace.id)
                                                }
                                                className="h-7 w-7 shrink-0 text-muted-foreground hover:text-destructive"
                                            >
                                                <Trash2 className="h-3.5 w-3.5" />
                                            </Button>
                                        </div>
                                    ))
                                )}
                            </div>

                            {error && (
                                <p className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-xs text-destructive">{error}</p>
                            )}
                        </CompactDialogBody>
                    </CompactDialogContent>
                </Dialog>
            </div>
        </div>
    );
};

export default AssistantWorkspaceFieldDisplay;
