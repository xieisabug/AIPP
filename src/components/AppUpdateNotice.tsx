import { useState } from "react";
import { ArrowUpFromLine, Download, Globe } from "lucide-react";
import type { AppUpdateStatus } from "@/data/Update";
import { checkAppUpdate, installAppUpdate, restartAfterAppUpdate, useAppUpdater } from "@/hooks/useAppUpdater";
import { Button } from "@/components/ui/button";
import { Progress } from "@/components/ui/progress";
import {
    Dialog,
    CompactDialogContent,
    CompactDialogHeader,
    CompactDialogBody,
} from "@/components/ui/dialog";

const phaseLabel: Record<AppUpdateStatus["phase"], string> = {
    idle: "", checking: "正在检查更新…", up_to_date: "已是最新版本",
    downloading: "正在下载", ready: "已下载，可安装", installing: "正在安装…",
    installed: "更新已安装，重启应用后生效", failed: "更新失败",
};

function byteSize(bytes: number) {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
}

function RestartUpdateButton() {
    return <Button type="button" size="sm" onClick={restartAfterAppUpdate}>重启 AIPP</Button>;
}

export function AppUpdateDetails({ status, canRestart }: { status: AppUpdateStatus; canRestart: boolean }) {
    const { phase, version, body, downloaded, total, error } = status;
    const busy = ["checking", "downloading", "installing", "installed"].includes(phase);
    const ready = phase === "ready" && !!version;
    return <div className="space-y-3">
        {phase !== "idle" && <p className="text-sm font-medium">
            {version && `v${version} · `}{phaseLabel[phase]}
        </p>}
        {phase === "downloading" && <div className="space-y-1">
            {/* 总大小未知时不给 value（Radix 视为 indeterminate），用整条轨道脉动表示进行中。 */}
            <Progress aria-label="下载进度" className={total ? undefined : "animate-pulse bg-primary/40"}
                value={total ? Math.min(100, downloaded / total * 100) : null} />
            <p className="text-xs tabular-nums text-muted-foreground">
                {total ? `${Math.min(100, Math.floor(downloaded / total * 100))}% · ${byteSize(downloaded)} / ${byteSize(total)}`
                    : `已下载 ${byteSize(downloaded)}`}
            </p>
        </div>}
        {error && <p role="alert" className="break-words text-sm text-destructive">{error}</p>}
        {body && <div className="max-h-48 overflow-auto whitespace-pre-wrap break-words rounded-lg bg-muted p-3 text-sm">{body}</div>}
        {canRestart && <RestartUpdateButton />}
        {!busy && <div className="flex gap-2">
            <Button type="button" variant={ready ? "default" : "outline"}
                onClick={() => ready ? installAppUpdate(version) : checkAppUpdate()}>
                {ready ? "立即安装" : phase === "failed" ? "重试" : "检查更新"}
            </Button>
            {!ready && <Button type="button" variant="outline" size="icon" aria-label="使用代理检查并下载更新"
                title="使用代理检查并下载更新" onClick={() => checkAppUpdate(true)}><Globe /></Button>}
        </div>}
    </div>;
}

export function AppUpdateNotice({ rounded = false }: { rounded?: boolean }) {
    const { available, status, canRestart } = useAppUpdater();
    const [open, setOpen] = useState(false);
    if (!available || !status?.version) return null;
    const version = status.version;
    const ready = status.phase === "ready";
    const title = `v${version} · ${ready ? "点击安装更新" : phaseLabel[status.phase]}`;
    return <>
        {canRestart ? <RestartUpdateButton /> : <Button type="button" size="icon" variant={rounded ? "outline" : "ghost"}
            className={`relative size-8 text-primary ${rounded ? "rounded-full" : ""}`}
            title={title} aria-label={title}
            onClick={() => ready ? installAppUpdate(version) : setOpen(true)}>
            {ready ? <ArrowUpFromLine /> : <Download />}
            <span aria-hidden className="absolute right-1 top-1 size-1.5 rounded-full bg-primary" />
        </Button>}
        <Dialog open={open} onOpenChange={setOpen}>
            <CompactDialogContent className="sm:max-w-md" aria-describedby={undefined}>
                <CompactDialogHeader title="应用更新" subtitle={version ? `v${version}` : undefined} />
                <CompactDialogBody>
                    <AppUpdateDetails status={status} canRestart={canRestart} />
                </CompactDialogBody>
            </CompactDialogContent>
        </Dialog>
    </>;
}
