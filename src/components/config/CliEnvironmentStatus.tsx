import React from "react";
import { Download, Loader2 } from "lucide-react";
import { Button } from "../ui/button";
import type { useAcpEnvironment } from "@/hooks/feature/useAcpEnvironment";
import type { DetectedProgram } from "@/hooks/feature/useNativeAgentEnvironment";
import { ProgramStatus } from "./native-agent/ProgramPicker";
import { UpdateControls } from "./native-agent/UpdatePanel";

type CliEnvironment = ReturnType<typeof useAcpEnvironment>;

interface CliEnvironmentStatusProps {
    /** 环境检测 Hook 的返回值（useAcpEnvironment 按 cliCommand 实例化） */
    env: CliEnvironment;
    /** 兜底展示用的 CLI 命令名 */
    cliCommand: string;
    /** 展示名称，例如 "Codex CLI" / "Kimi Code CLI" */
    displayName: string;
}

/** `--version` 原样输出（如 "2.1.0 (Claude Code)"）里取出版本号；取不到时保留原文。 */
export function versionNumber(raw: string | null | undefined): string | null {
    const text = raw?.trim();
    if (!text) return null;
    return text.match(/\d+(?:\.\d+)+(?:[-+][0-9A-Za-z.-]+)?/)?.[0] ?? text;
}

/** 与原生智能体安装指引相同的中性提示框。 */
const NoticeBox: React.FC<{ children: React.ReactNode }> = ({ children }) => (
    <div className="flex flex-col gap-2 rounded-md border border-border bg-muted/40 p-3">{children}</div>
);

/**
 * Codex / Claude Code / ACP（Kimi、Gemini 等）的本机 CLI 环境状态。
 *
 * 与 Pi、Cursor 等原生智能体共用展示：一行「版本 · 路径」、中性的安装提示框，
 * 以及底部的检查更新 / 重新检测（UpdateControls）。数据仍来自 useAcpEnvironment。
 */
export const CliEnvironmentStatus: React.FC<CliEnvironmentStatusProps> = ({ env, cliCommand, displayName }) => {
    const {
        status,
        libraryInfo,
        installLog,
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
    // 切换 CLI 时旧的检测结果不属于当前命令，不能拿来展示
    const info = libraryInfo?.cli_command === cliCommand ? libraryInfo : null;
    const installed = Boolean(info?.installed);
    const version = versionNumber(info?.version);
    const detecting = status === "checking";

    const controls = (
        <UpdateControls
            displayName={displayName}
            canUpdate={installed && Boolean(info?.can_update)}
            currentVersion={version}
            latestVersion={latestVersion}
            upToDate={hasCheckedUpdate && !latestVersion}
            checking={isCheckingUpdate}
            checkError={checkUpdateError}
            canCheckWithProxy={canRetryCheckWithProxy}
            onCheck={(useProxy) => void checkAcpLibraryUpdate(useProxy)}
            updating={status === "updating"}
            updateLog={installLog}
            updateError={installed ? updateError : null}
            canUpdateWithProxy={canRetryUpdateWithProxy}
            onUpdate={(useProxy) => void updateAcpLibrary(useProxy)}
            detecting={detecting}
            onDetect={() => void checkAcpLibrary()}
        />
    );

    // 已安装时重新检测保留当前结果（只转动「重新检测」图标），避免整块收起再展开
    if (installed && (status === "installed" || status === "updating" || detecting)) {
        const program: DetectedProgram = {
            path: info?.installed_path || cliCommand,
            source: "",
            version,
            ok: true,
            warning: null,
            error: null,
        };
        return (
            <div className="flex flex-col gap-2">
                <ProgramStatus program={program} />
                {info?.update_hint && <p className="text-xs text-muted-foreground">{info.update_hint}</p>}
                {controls}
            </div>
        );
    }

    if (detecting) return <ProgramStatus program={null} detecting />;

    if (status === "installing") {
        return (
            <NoticeBox>
                <span className="flex items-center gap-2 text-sm">
                    <Loader2 className="h-4 w-4 animate-spin" />正在安装 {displayName}…
                </span>
                <pre className="max-h-40 overflow-auto whitespace-pre-wrap break-all rounded bg-background p-2 text-xs" aria-label="安装日志">
                    {installLog || "等待输出…"}
                </pre>
            </NoticeBox>
        );
    }

    const hint = info?.install_hint;
    const missing: DetectedProgram = {
        path: cliCommand,
        source: "",
        version: null,
        ok: false,
        warning: null,
        error: env.probeError || `未找到 ${displayName}`,
    };

    return (
        <div className="flex flex-col gap-2">
            <ProgramStatus program={missing} />
            {status === "not-installed" && (
                <NoticeBox>
                    <p className="text-xs text-muted-foreground">
                        {hint || `可通过 Bun 一键安装 ${info?.package_name || cliCommand}。`}
                    </p>
                    <Button type="button" size="sm" className="w-fit" onClick={() => void installAcpLibrary()}>
                        <Download className="mr-1 h-4 w-4" />一键安装
                    </Button>
                </NoticeBox>
            )}
            {status === "bun-not-installed" && (
                <NoticeBox>
                    <p className="text-xs text-muted-foreground">
                        一键安装 {displayName} 需要 Bun 运行时，请先在「设置 → 预览配置」中安装 Bun。
                    </p>
                    <Button
                        type="button"
                        variant="outline"
                        size="sm"
                        className="w-fit"
                        onClick={async () => {
                            const { emit } = await import("@tauri-apps/api/event");
                            await emit("config-navigate-to", { menu: "feature-assistant-config", subNav: "preview" });
                        }}
                    >
                        前往安装 Bun
                    </Button>
                </NoticeBox>
            )}
            {status === "external-required" && (
                <NoticeBox>
                    <p className="text-xs text-muted-foreground">
                        {hint || `请手动安装 ${cliCommand}`}；安装后点击「重新检测」。
                    </p>
                </NoticeBox>
            )}
            {controls}
        </div>
    );
};

export default CliEnvironmentStatus;
