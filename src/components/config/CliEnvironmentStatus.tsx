import React from "react";
import { Button } from "../ui/button";
import { AlertTriangle, CheckCircle2, Loader2, XCircle } from "lucide-react";
import type { useAcpEnvironment } from "@/hooks/feature/useAcpEnvironment";

type CliEnvironment = ReturnType<typeof useAcpEnvironment>;

interface CliEnvironmentStatusProps {
    /** 环境检测 Hook 的返回值（useAcpEnvironment 按 cliCommand 实例化） */
    env: CliEnvironment;
    /** 兜底展示用的 CLI 命令名 */
    cliCommand: string;
    /** 展示名称，例如 "ACP 库" / "Codex CLI" */
    displayName: string;
}

/**
 * Agent CLI 环境状态卡片
 *
 * 展示检测结果（版本、安装路径）、一键安装/更新入口与安装日志。
 * ACP 适配器与官方 Codex CLI 共用，差异仅在 displayName 文案。
 */
export const CliEnvironmentStatus: React.FC<CliEnvironmentStatusProps> = ({
    env,
    cliCommand,
    displayName,
}) => {
    const {
        status,
        libraryInfo,
        installAcpLibrary,
        updateAcpLibrary,
        checkAcpLibrary,
        checkAcpLibraryUpdate,
        isCheckingUpdate,
        latestVersion,
        hasCheckedUpdate,
        checkUpdateError,
        updateError,
        canRetryCheckWithProxy,
        canRetryUpdateWithProxy,
    } = env;

    switch (status) {
        case "checking":
            return (
                <div className="p-3 border border-border rounded-lg bg-muted/50">
                    <div className="flex items-center gap-2 text-muted-foreground">
                        <Loader2 className="h-4 w-4 animate-spin" />
                        <span className="text-sm">正在检测环境...</span>
                    </div>
                </div>
            );

        case "bun-not-installed":
            return (
                <div className="p-3 border border-destructive/50 rounded-lg bg-destructive/10">
                    <div className="flex items-center gap-2 text-destructive mb-2">
                        <XCircle className="h-4 w-4" />
                        <span className="text-sm font-medium">Bun 运行时未安装</span>
                    </div>
                    <p className="text-xs text-muted-foreground mb-2">
                        {displayName} 的自动安装依赖 Bun 运行时。请前往【设置 → 预览配置】安装 Bun。
                    </p>
                    <Button
                        variant="outline"
                        size="sm"
                        onClick={async () => {
                            const { emit } = await import("@tauri-apps/api/event");
                            await emit("config-navigate-to", { menu: "feature-assistant-config", subNav: "preview" });
                        }}
                    >
                        前往安装 Bun
                    </Button>
                </div>
            );

        case "not-installed":
            return (
                <div className="p-3 border border-yellow-500/50 rounded-lg bg-yellow-500/10">
                    <div className="flex items-center gap-2 text-yellow-600 dark:text-yellow-400 mb-2">
                        <AlertTriangle className="h-4 w-4" />
                        <span className="text-sm font-medium">{displayName} 未安装</span>
                    </div>
                    <p className="text-xs text-muted-foreground mb-2">
                        需要安装 {libraryInfo?.package_name || cliCommand} 才能使用此功能。
                        {libraryInfo?.install_hint && (
                            <span className="block mt-1 text-yellow-600 dark:text-yellow-400">
                                提示: {libraryInfo.install_hint}
                            </span>
                        )}
                    </p>
                    <Button
                        variant="default"
                        size="sm"
                        onClick={() => installAcpLibrary()}
                    >
                        一键安装
                    </Button>
                </div>
            );

        case "installing":
        case "updating":
            return (
                <div className="p-3 border border-border rounded-lg bg-muted/50">
                    <div className="flex items-center gap-2 text-muted-foreground mb-2">
                        <Loader2 className="h-4 w-4 animate-spin" />
                        <span className="text-sm font-medium">
                            {status === "updating" ? "正在更新..." : "正在安装..."}
                        </span>
                    </div>
                    <pre className="text-xs bg-background p-2 rounded max-h-32 overflow-auto whitespace-pre-wrap">
                        {env.installLog || "等待安装日志..."}
                    </pre>
                </div>
            );

        case "external-required":
            return (
                <div className="p-3 border border-yellow-500/50 rounded-lg bg-yellow-500/10">
                    <div className="flex items-center gap-2 text-yellow-600 dark:text-yellow-400 mb-2">
                        <AlertTriangle className="h-4 w-4" />
                        <span className="text-sm font-medium">需要手动安装</span>
                    </div>
                    <p className="text-xs text-muted-foreground mb-2">
                        {libraryInfo?.install_hint || `请手动安装 ${cliCommand}`}
                    </p>
                    <Button
                        variant="outline"
                        size="sm"
                        onClick={() => checkAcpLibrary()}
                    >
                        重新检测
                    </Button>
                </div>
            );

        case "installed":
            return (
                <div className="p-3 border border-green-500/50 rounded-lg bg-green-500/10">
                    <div className="flex items-center gap-2 text-green-600 dark:text-green-400">
                        <CheckCircle2 className="h-4 w-4" />
                        <span className="text-sm font-medium">环境就绪</span>
                        {libraryInfo?.version && (
                            <span className="text-xs text-muted-foreground">
                                (v{libraryInfo.version})
                            </span>
                        )}
                    </div>
                    {libraryInfo?.installed_path && (
                        <div className="mt-2">
                            <p className="text-xs text-muted-foreground mb-1">已安装位置</p>
                            <pre className="text-xs bg-background/80 p-2 rounded overflow-auto whitespace-pre-wrap break-all">
                                {libraryInfo.installed_path}
                            </pre>
                        </div>
                    )}
                    {libraryInfo?.install_hint && (
                        <p className="text-xs text-muted-foreground mt-1">
                            提示: {libraryInfo.install_hint}
                        </p>
                    )}
                    <div className="mt-2">
                        {isCheckingUpdate && (
                            <div className="flex items-center gap-2 text-xs text-muted-foreground">
                                <Loader2 className="h-3.5 w-3.5 animate-spin" />
                                <span>正在检查更新...</span>
                            </div>
                        )}
                        {!isCheckingUpdate && latestVersion && (
                            <p className="text-xs text-amber-700 dark:text-amber-300">
                                发现可用更新: v{latestVersion}
                            </p>
                        )}
                        {!isCheckingUpdate && hasCheckedUpdate && !latestVersion && (
                            <p className="text-xs text-green-700 dark:text-green-300">
                                当前已是最新版本
                            </p>
                        )}
                        {!isCheckingUpdate && checkUpdateError && (
                            <p className="text-xs text-amber-700 dark:text-amber-300">
                                {checkUpdateError}
                            </p>
                        )}
                        {updateError && (
                            <p className="text-xs text-amber-700 dark:text-amber-300 mt-1">
                                {updateError}
                            </p>
                        )}
                    </div>
                    <div className="mt-3 flex flex-wrap gap-2">
                        <Button
                            variant="outline"
                            size="sm"
                            onClick={() => checkAcpLibrary()}
                            disabled={isCheckingUpdate}
                        >
                            重新检测
                        </Button>
                        {!libraryInfo?.requires_external_install && (
                            <Button
                                variant="outline"
                                size="sm"
                                onClick={() => checkAcpLibraryUpdate()}
                                disabled={isCheckingUpdate}
                            >
                                {isCheckingUpdate && <Loader2 className="h-4 w-4 mr-2 animate-spin" />}
                                检查更新
                            </Button>
                        )}
                        {canRetryCheckWithProxy && !libraryInfo?.requires_external_install && (
                            <Button
                                variant="outline"
                                size="sm"
                                onClick={() => checkAcpLibraryUpdate(true)}
                                disabled={isCheckingUpdate}
                            >
                                使用代理检查更新
                            </Button>
                        )}
                        {latestVersion && (
                            <Button
                                variant="default"
                                size="sm"
                                onClick={() => updateAcpLibrary()}
                                disabled={isCheckingUpdate}
                            >
                                更新到 v{latestVersion}
                            </Button>
                        )}
                        {latestVersion && canRetryUpdateWithProxy && (
                            <Button
                                variant="outline"
                                size="sm"
                                onClick={() => updateAcpLibrary(true)}
                                disabled={isCheckingUpdate}
                            >
                                使用代理更新到 v{latestVersion}
                            </Button>
                        )}
                    </div>
                </div>
            );

        default:
            return null;
    }
};

export default CliEnvironmentStatus;
