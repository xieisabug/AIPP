import React from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { toast } from "sonner";
import { ArrowUpCircle, Copy, ExternalLink, Loader2, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import type { NativeAgentEnvironmentState } from "@/hooks/feature/useNativeAgentEnvironment";

/** Official install commands for a missing agent program, with copy buttons. */
export const InstallGuide: React.FC<{
    commands: { label: string; command: string }[];
    docsUrl: string;
    intro: string;
}> = ({ commands, docsUrl, intro }) => (
    <div className="flex flex-col gap-2 rounded-md border border-border bg-muted/40 p-3">
        <p className="text-xs text-muted-foreground">{intro}</p>
        {commands.map((item) => (
            <div key={item.command} className="flex items-center gap-2">
                <span className="w-16 shrink-0 text-xs text-muted-foreground">{item.label}</span>
                <code className="min-w-0 flex-1 truncate rounded bg-background px-2 py-1 font-mono text-xs" title={item.command}>
                    {item.command}
                </code>
                <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    className="h-7 w-7 shrink-0"
                    aria-label={`复制 ${item.command}`}
                    onClick={() => {
                        void navigator.clipboard.writeText(item.command).then(
                            () => toast.success("已复制安装命令"),
                            (error) => toast.error(`复制失败：${String(error)}`),
                        );
                    }}
                >
                    <Copy className="h-3.5 w-3.5" />
                </Button>
            </div>
        ))}
        {docsUrl && (
            <button
                type="button"
                className="flex w-fit items-center gap-1 text-xs text-muted-foreground underline-offset-2 hover:text-foreground hover:underline"
                onClick={() => void openUrl(docsUrl).catch((error) => toast.error(String(error)))}
            >
                <ExternalLink className="h-3 w-3" />官方文档
            </button>
        )}
    </div>
);

export interface UpdateControlsProps {
    displayName: string;
    /** AIPP can check and run updates for this program. */
    canUpdate: boolean;
    currentVersion: string | null;
    /** A newer published release, when one is known. */
    latestVersion: string | null;
    /** A check finished and found nothing newer. */
    upToDate: boolean;
    checking: boolean;
    checkError: string | null;
    /** Offer a check through the global proxy (after a direct check failed). */
    canCheckWithProxy: boolean;
    onCheck: (useProxy: boolean) => void;
    updating: boolean;
    updateLog: string;
    updateError: string | null;
    canUpdateWithProxy: boolean;
    onUpdate: (useProxy: boolean) => void;
    detecting: boolean;
    onDetect: () => void;
}

const LINK = "underline-offset-2 hover:text-foreground hover:underline disabled:opacity-50";

/**
 * Version freshness for the detected program: a newer release shows an "update" action,
 * otherwise a quiet "up to date" line with manual re-check. Shared by every local agent
 * provider form (native agents, Codex, Claude Code, ACP) so they look the same.
 */
export const UpdateControls: React.FC<UpdateControlsProps> = ({
    displayName, canUpdate, currentVersion, latestVersion, upToDate, checking, checkError, canCheckWithProxy, onCheck,
    updating, updateLog, updateError, canUpdateWithProxy, onUpdate, detecting, onDetect,
}) => (
    <div className="flex flex-col gap-2">
        {canUpdate && latestVersion && !updating && (
            <div className="flex flex-wrap items-center justify-between gap-2 rounded-md border border-border bg-muted/40 px-3 py-2">
                <span className="flex items-center gap-2 text-sm">
                    <ArrowUpCircle className="h-4 w-4 text-muted-foreground" />
                    新版本 v{latestVersion} 可用
                    {currentVersion && <span className="text-xs text-muted-foreground">当前 v{currentVersion}</span>}
                </span>
                <Button type="button" size="sm" onClick={() => onUpdate(false)}>更新</Button>
            </div>
        )}
        {updating && (
            <div className="flex flex-col gap-2 rounded-md border border-border bg-muted/40 p-3">
                <span className="flex items-center gap-2 text-sm">
                    <Loader2 className="h-4 w-4 animate-spin" />正在更新 {displayName}…
                </span>
                <pre className="max-h-40 overflow-auto whitespace-pre-wrap break-all rounded bg-background p-2 text-xs" aria-label="更新日志">
                    {updateLog || "等待输出…"}
                </pre>
            </div>
        )}
        {!updating && updateError && (
            <div className="flex flex-col gap-2 rounded-md border border-destructive/40 p-3">
                <p className="whitespace-pre-wrap break-all text-xs text-destructive">{updateError}</p>
                {updateLog && (
                    <pre className="max-h-32 overflow-auto whitespace-pre-wrap break-all rounded bg-muted p-2 text-xs">{updateLog}</pre>
                )}
                <div className="flex flex-wrap gap-2">
                    <Button type="button" size="sm" variant="outline" onClick={() => onUpdate(false)}>重试</Button>
                    {canUpdateWithProxy && (
                        <Button type="button" size="sm" variant="outline" onClick={() => onUpdate(true)}>使用全局代理重试</Button>
                    )}
                </div>
            </div>
        )}
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-muted-foreground">
            {canUpdate && checking && (
                <span className="flex items-center gap-1"><Loader2 className="h-3 w-3 animate-spin" />正在检查更新…</span>
            )}
            {canUpdate && !checking && upToDate && <span>已是最新版本</span>}
            {canUpdate && !checking && checkError && <span className="break-all text-destructive">{checkError}</span>}
            {canUpdate && (
                <button type="button" className={LINK} disabled={checking || updating} onClick={() => onCheck(false)}>
                    检查更新
                </button>
            )}
            {canUpdate && canCheckWithProxy && (
                <button type="button" className={LINK} disabled={checking || updating} onClick={() => onCheck(true)}>
                    使用全局代理检查
                </button>
            )}
            <button type="button" className={`flex items-center gap-1 ${LINK}`} disabled={detecting || updating} onClick={onDetect}>
                <RefreshCw className={`h-3 w-3 ${detecting ? "animate-spin" : ""}`} />重新检测
            </button>
        </div>
    </div>
);

interface UpdatePanelProps {
    state: NativeAgentEnvironmentState;
    displayName: string;
    /** Called with the Cursor SDK directory after a successful run. */
    onUpdated?: (directory: string | null) => void;
}

/** UpdateControls driven by the native agent environment (the agent's own updater). */
export const UpdatePanel: React.FC<UpdatePanelProps> = ({ state, displayName, onUpdated }) => {
    const { environment, currentVersion, updateInfo, checkingUpdate, checkError, checkUpdate, updating, updateLog, updateError, runUpdate, detect, detecting } = state;

    const update = async (useProxy: boolean) => {
        try {
            const directory = await runUpdate(useProxy);
            toast.success(`${displayName} 已更新`);
            onUpdated?.(directory);
        } catch (error) {
            toast.error(`${displayName} 更新失败`, { description: String(error).slice(0, 300) });
        }
    };

    return (
        <UpdateControls
            displayName={displayName}
            canUpdate={Boolean(environment?.tool.can_update && currentVersion)}
            currentVersion={currentVersion}
            latestVersion={updateInfo?.update_available ? updateInfo.latest : null}
            upToDate={Boolean(updateInfo && !updateInfo.update_available)}
            checking={checkingUpdate}
            checkError={checkError}
            canCheckWithProxy={Boolean(checkError)}
            onCheck={(useProxy) => void checkUpdate(useProxy ? { useProxy: true } : { force: true })}
            updating={updating}
            updateLog={updateLog}
            updateError={updateError}
            canUpdateWithProxy
            onUpdate={(useProxy) => void update(useProxy)}
            detecting={detecting}
            onDetect={() => void detect()}
        />
    );
};
