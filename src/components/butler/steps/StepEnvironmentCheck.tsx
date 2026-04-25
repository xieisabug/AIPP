import React, { useEffect } from "react";
import { Button } from "@/components/ui/button";
import { CheckCircle2, XCircle, Loader2, Download, Terminal } from "lucide-react";

interface StepEnvironmentCheckProps {
    bunVersion: string | null;
    uvVersion: string | null;
    bunInstalling: boolean;
    uvInstalling: boolean;
    bunInstallLog: string;
    uvInstallLog: string;
    onCheckBun: () => Promise<boolean>;
    onCheckUv: () => Promise<boolean>;
    onInstallBun: () => Promise<void>;
    onInstallUv: () => Promise<void>;
}

const StepEnvironmentCheck: React.FC<StepEnvironmentCheckProps> = ({
    bunVersion,
    uvVersion,
    bunInstalling,
    uvInstalling,
    bunInstallLog,
    uvInstallLog,
    onCheckBun,
    onCheckUv,
    onInstallBun,
    onInstallUv,
}) => {
    useEffect(() => {
        void onCheckBun();
        void onCheckUv();
    }, [onCheckBun, onCheckUv]);

    return (
        <div className="space-y-6">
            <div className="space-y-2">
                <div className="flex items-center gap-2 text-primary">
                    <Terminal className="h-5 w-5" />
                    <h3 className="text-lg font-semibold">环境检测</h3>
                </div>
                <p className="text-sm text-muted-foreground leading-relaxed">
                    总管家可以执行脚本来完成复杂任务。以下工具可以增强它的脚本执行能力，
                    但并非必须安装。你可以随时跳过此步骤，稍后再安装。
                </p>
            </div>

            <div className="space-y-3">
                {/* Bun */}
                <ToolCard
                    name="Bun"
                    description="快速的 JavaScript 运行时和包管理器，用于运行 React/Vue 组件预览和前端脚本。"
                    version={bunVersion}
                    installing={bunInstalling}
                    installLog={bunInstallLog}
                    onInstall={onInstallBun}
                    onRecheck={onCheckBun}
                />

                {/* UV */}
                <ToolCard
                    name="uv"
                    description="极快的 Python 包管理器，用于管理 Python 项目依赖和执行 Python 脚本。"
                    version={uvVersion}
                    installing={uvInstalling}
                    installLog={uvInstallLog}
                    onInstall={onInstallUv}
                    onRecheck={onCheckUv}
                />
            </div>
        </div>
    );
};

interface ToolCardProps {
    name: string;
    description: string;
    version: string | null;
    installing: boolean;
    installLog: string;
    onInstall: () => Promise<void>;
    onRecheck: () => Promise<boolean>;
}

const ToolCard: React.FC<ToolCardProps> = ({
    name,
    description,
    version,
    installing,
    installLog,
    onInstall,
    onRecheck,
}) => {
    const isInstalled = version !== null;

    return (
        <div className="rounded-lg border border-border/60 bg-muted/30 p-4 space-y-3">
            <div className="flex items-start justify-between gap-3">
                <div className="flex-1 space-y-1">
                    <div className="flex items-center gap-2">
                        <span className="font-medium">{name}</span>
                        {isInstalled ? (
                            <span className="inline-flex items-center gap-1 text-xs text-green-600 dark:text-green-400">
                                <CheckCircle2 className="h-3.5 w-3.5" />
                                {version}
                            </span>
                        ) : (
                            <span className="inline-flex items-center gap-1 text-xs text-muted-foreground">
                                <XCircle className="h-3.5 w-3.5" />
                                未安装
                            </span>
                        )}
                    </div>
                    <p className="text-xs text-muted-foreground">{description}</p>
                </div>

                <div className="shrink-0">
                    {installing ? (
                        <Button variant="outline" size="sm" disabled>
                            <Loader2 className="h-3.5 w-3.5 mr-1.5 animate-spin" />
                            安装中
                        </Button>
                    ) : isInstalled ? (
                        <Button variant="ghost" size="sm" onClick={() => void onRecheck()}>
                            重新检测
                        </Button>
                    ) : (
                        <Button variant="default" size="sm" onClick={() => void onInstall()}>
                            <Download className="h-3.5 w-3.5 mr-1.5" />
                            一键安装
                        </Button>
                    )}
                </div>
            </div>

            {installing && installLog && (
                <div className="rounded-md bg-background/80 border border-border/40 p-2 max-h-32 overflow-y-auto">
                    <pre className="text-[11px] font-mono text-muted-foreground whitespace-pre-wrap break-all">
                        {installLog}
                    </pre>
                </div>
            )}
        </div>
    );
};

export default StepEnvironmentCheck;
