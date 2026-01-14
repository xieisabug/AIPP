import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";

/** ACP 库信息 */
export interface AcpLibraryInfo {
    /** CLI 命令名称 */
    cli_command: string;
    /** 对应的 npm 包名 */
    package_name: string;
    /** 是否已安装 */
    installed: boolean;
    /** 安装的版本（如果已安装） */
    version: string | null;
    /** 是否需要外部安装（如 gemini 需要用户自行安装） */
    requires_external_install: boolean;
    /** 安装说明 */
    install_hint: string;
}

/** ACP 安装完成事件 payload */
interface AcpInstallFinishedPayload {
    success: boolean;
    cli_command: string;
    package_name: string;
}

/** ACP 环境状态 */
export type AcpEnvironmentStatus =
    | "checking"           // 正在检测
    | "bun-not-installed"  // Bun 未安装
    | "not-installed"      // ACP 库未安装
    | "installing"         // 正在安装
    | "installed"          // 已安装
    | "external-required"; // 需要外部安装

/**
 * ACP 环境管理 Hook
 * 用于检测和安装 ACP CLI 工具
 */
export const useAcpEnvironment = (cliCommand: string) => {
    const [status, setStatus] = useState<AcpEnvironmentStatus>("checking");
    const [libraryInfo, setLibraryInfo] = useState<AcpLibraryInfo | null>(null);
    const [installLog, setInstallLog] = useState<string>("");
    const [bunVersion, setBunVersion] = useState<string>("");

    // 检查 Bun 版本
    const checkBunVersion = useCallback(async () => {
        try {
            const version = await invoke<string>("check_bun_version");
            setBunVersion(version);
            return version !== "Not Installed" && version !== "";
        } catch {
            setBunVersion("Not Installed");
            return false;
        }
    }, []);

    // 检查 ACP 库状态
    const checkAcpLibrary = useCallback(async () => {
        if (!cliCommand) {
            setStatus("checking");
            return;
        }

        setStatus("checking");

        try {
            const info = await invoke<AcpLibraryInfo>("check_acp_library", {
                cliCommand,
            });
            setLibraryInfo(info);

            if (info.requires_external_install) {
                if (info.installed) {
                    setStatus("installed");
                } else {
                    setStatus("external-required");
                }
            } else if (info.installed) {
                setStatus("installed");
            } else {
                // 检查 Bun 是否可用
                const bunAvailable = await checkBunVersion();
                if (!bunAvailable) {
                    setStatus("bun-not-installed");
                } else {
                    setStatus("not-installed");
                }
            }
        } catch (err) {
            console.error("检查 ACP 库失败:", err);
            setStatus("not-installed");
        }
    }, [cliCommand, checkBunVersion]);

    // 安装 ACP 库
    const installAcpLibrary = useCallback(async () => {
        if (!cliCommand) return;

        setStatus("installing");
        setInstallLog("开始安装...");

        try {
            await invoke("install_acp_library", { cliCommand });
        } catch (err) {
            console.error("安装 ACP 库失败:", err);
            toast.error(`安装失败: ${err}`);
            setStatus("not-installed");
        }
    }, [cliCommand]);

    // 监听安装事件
    useEffect(() => {
        const unlistenLog = listen<string>("acp-install-log", (event) => {
            setInstallLog((prev) => prev + "\n" + event.payload);
        });

        const unlistenFinished = listen<AcpInstallFinishedPayload>(
            "acp-install-finished",
            (event) => {
                if (event.payload.cli_command === cliCommand) {
                    if (event.payload.success) {
                        toast.success(`${event.payload.package_name} 安装成功`);
                        // 重新检查状态
                        checkAcpLibrary();
                    } else {
                        toast.error(`${event.payload.package_name} 安装失败`);
                        setStatus("not-installed");
                    }
                }
            }
        );

        return () => {
            unlistenLog.then((f) => f());
            unlistenFinished.then((f) => f());
        };
    }, [cliCommand, checkAcpLibrary]);

    // CLI 命令变化时重新检查
    useEffect(() => {
        if (cliCommand) {
            checkAcpLibrary();
        }
    }, [cliCommand, checkAcpLibrary]);

    return {
        /** 当前状态 */
        status,
        /** 库信息 */
        libraryInfo,
        /** 安装日志 */
        installLog,
        /** Bun 版本 */
        bunVersion,
        /** 重新检查环境 */
        checkAcpLibrary,
        /** 安装 ACP 库 */
        installAcpLibrary,
    };
};
