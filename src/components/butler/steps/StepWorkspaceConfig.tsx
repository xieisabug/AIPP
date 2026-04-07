import React, { useCallback, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { FolderPicker } from "@/components/config/FolderPicker";
import {
    AlertDialog,
    AlertDialogAction,
    AlertDialogCancel,
    AlertDialogContent,
    AlertDialogDescription,
    AlertDialogFooter,
    AlertDialogHeader,
    AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { AlertTriangle, FolderOpen, Plus, Trash2, ShieldAlert } from "lucide-react";
import {
    BUTLER_MAIN_WORKSPACE_DEFAULT_DESCRIPTION,
    type TrustedWorkspace,
} from "../butlerWorkspaceConfig";

interface StepWorkspaceConfigProps {
    mainWorkspacePath: string;
    mainWorkspaceDescription: string;
    trustedWorkspaces: TrustedWorkspace[];
    trustAllWorkspaces: boolean;
    onMainWorkspacePathChange: (path: string) => void;
    onMainWorkspaceDescriptionChange: (description: string) => void;
    onTrustAllChange: (trustAll: boolean) => void;
    onAddWorkspace: (path: string, description: string) => void;
    onRemoveWorkspace: (path: string) => void;
    onUpdateDescription: (path: string, description: string) => void;
}

const StepWorkspaceConfig: React.FC<StepWorkspaceConfigProps> = ({
    mainWorkspacePath,
    mainWorkspaceDescription,
    trustedWorkspaces,
    trustAllWorkspaces,
    onMainWorkspacePathChange,
    onMainWorkspaceDescriptionChange,
    onTrustAllChange,
    onAddWorkspace,
    onRemoveWorkspace,
    onUpdateDescription,
}) => {
    const [newPath, setNewPath] = useState("");
    const [newDesc, setNewDesc] = useState("");
    const [showTrustAllConfirm, setShowTrustAllConfirm] = useState(false);

    const handleAddPath = useCallback(() => {
        if (newPath.trim()) {
            onAddWorkspace(newPath.trim(), newDesc.trim());
            setNewPath("");
            setNewDesc("");
        }
    }, [newPath, newDesc, onAddWorkspace]);

    const handleTrustAllToggle = useCallback(
        (checked: boolean) => {
            if (checked) {
                setShowTrustAllConfirm(true);
            } else {
                onTrustAllChange(false);
            }
        },
        [onTrustAllChange]
    );

    return (
        <div className="space-y-6">
            <div className="space-y-2">
                <div className="flex items-center gap-2 text-primary">
                    <FolderOpen className="h-5 w-5" />
                    <h3 className="text-lg font-semibold">配置工作区</h3>
                </div>
                <p className="text-sm text-muted-foreground leading-relaxed">
                    工作区是总管家可以自动读写文件的目录范围。在可信工作区内，文件操作会自动放行，
                    不需要逐次确认权限。工作区的描述会注入到系统提示词中，帮助 AI 理解每个项目的用途。
                </p>
            </div>

            {/* Trust All toggle */}
            <div className="rounded-lg border border-destructive/30 bg-destructive/5 p-4">
                <div className="flex items-center justify-between gap-4">
                    <div className="space-y-1">
                        <div className="flex items-center gap-2">
                            <span className="text-sm font-medium">信任任何工作区</span>
                            <span className="inline-flex items-center gap-1 rounded-md bg-destructive/10 px-2 py-0.5 text-xs font-medium text-destructive">
                                <AlertTriangle className="h-3 w-3" />
                                危险
                            </span>
                        </div>
                        <p className="text-xs text-muted-foreground">
                            开启后总管家对所有路径的文件操作将自动放行，不再弹出任何权限确认弹窗。
                            仅建议在完全受信的个人开发环境下使用。
                        </p>
                    </div>
                    <Switch
                        checked={trustAllWorkspaces}
                        onCheckedChange={handleTrustAllToggle}
                    />
                </div>
            </div>

            <div className="space-y-3 rounded-lg border border-border/60 bg-muted/30 p-4">
                <div className="space-y-3 rounded-lg border border-primary/20 bg-background p-4">
                    <p className="text-sm font-medium">主工作区</p>
                    <p className="text-xs text-muted-foreground">
                        主工作区为必填。总管家会优先把任务组织到这里；未填写描述时默认使用预设说明。
                    </p>
                    <FolderPicker
                        value={mainWorkspacePath}
                        onChange={onMainWorkspacePathChange}
                        placeholder="选择或输入主工作区目录路径"
                    />
                    <Input
                        value={mainWorkspaceDescription}
                        onChange={(e) => onMainWorkspaceDescriptionChange(e.target.value)}
                        placeholder={BUTLER_MAIN_WORKSPACE_DEFAULT_DESCRIPTION}
                        className="text-sm"
                    />
                    {!mainWorkspacePath.trim() ? (
                        <p className="text-xs text-destructive">请先配置主工作区</p>
                    ) : null}
                </div>

                {!trustAllWorkspaces && (
                    <>
                        <p className="text-sm font-medium text-muted-foreground">额外可信工作区</p>

                    <div className="space-y-2">
                        <div className="flex items-center gap-2">
                            <FolderPicker
                                value={newPath}
                                onChange={setNewPath}
                                placeholder="选择或输入可信目录路径"
                            />
                            <Button
                                type="button"
                                size="sm"
                                onClick={handleAddPath}
                                disabled={!newPath.trim()}
                            >
                                <Plus className="h-4 w-4 mr-1" />
                                添加
                            </Button>
                        </div>
                        <Input
                            value={newDesc}
                            onChange={(e) => setNewDesc(e.target.value)}
                            placeholder="工作区描述（可选），例如：前端项目、Rust 后端代码仓库"
                            className="text-sm"
                        />
                    </div>

                    <div className="space-y-2 max-h-48 overflow-y-auto">
                        {trustedWorkspaces.length === 0 ? (
                            <div className="text-sm text-muted-foreground text-center py-4">
                                暂未配置额外可信工作区。主工作区以外的目录可按需补充。
                            </div>
                        ) : (
                            trustedWorkspaces.map((ws) => (
                                <div
                                    key={ws.path}
                                    className="p-2 bg-background rounded-md border space-y-1"
                                >
                                    <div className="flex items-center justify-between">
                                        <span className="text-sm font-mono break-all flex-1 mr-2">
                                            {ws.path}
                                        </span>
                                        <Button
                                            type="button"
                                            variant="ghost"
                                            size="sm"
                                            onClick={() => onRemoveWorkspace(ws.path)}
                                            className="text-destructive hover:text-destructive shrink-0"
                                        >
                                            <Trash2 className="h-4 w-4" />
                                        </Button>
                                    </div>
                                    <Input
                                        value={ws.description}
                                        onChange={(e) =>
                                            onUpdateDescription(ws.path, e.target.value)
                                        }
                                        placeholder="添加描述…"
                                        className="text-xs h-7"
                                    />
                                </div>
                            ))
                        )}
                    </div>
                    </>
                )}
            </div>

            {/* Trust-all confirmation dialog */}
            <AlertDialog open={showTrustAllConfirm} onOpenChange={setShowTrustAllConfirm}>
                <AlertDialogContent>
                    <AlertDialogHeader>
                        <AlertDialogTitle className="flex items-center gap-2">
                            <ShieldAlert className="h-5 w-5 text-destructive" />
                            确认信任所有工作区？
                        </AlertDialogTitle>
                        <AlertDialogDescription className="space-y-2">
                            <p>
                                开启此选项后，总管家将对<strong>任意路径</strong>的文件操作自动放行，
                                包括读取、写入、删除等操作，不再弹出权限确认弹窗。
                            </p>
                            <p className="text-destructive font-medium">
                                ⚠️ 这意味着 AI 可以不经确认地修改你电脑上的任何文件，
                                请确保你了解相关风险。
                            </p>
                        </AlertDialogDescription>
                    </AlertDialogHeader>
                    <AlertDialogFooter>
                        <AlertDialogCancel>取消</AlertDialogCancel>
                        <AlertDialogAction
                            className="bg-destructive hover:bg-destructive/90"
                            onClick={() => onTrustAllChange(true)}
                        >
                            我了解风险，确认开启
                        </AlertDialogAction>
                    </AlertDialogFooter>
                </AlertDialogContent>
            </AlertDialog>
        </div>
    );
};

export default StepWorkspaceConfig;
