import { useState, useEffect, useCallback, useRef } from "react";
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
    /** 已安装 CLI 的解析路径（如果已安装） */
    installed_path: string | null;
    /** 是否需要外部安装（如 gemini 需要用户自行安装） */
    requires_external_install: boolean;
    /** 安装说明 */
    install_hint: string;
}

/** ACP 安装完成事件 payload */
interface AcpInstallFinishedPayload {
    success: boolean;
    action?: "install" | "update";
    used_proxy?: boolean;
    cli_command: string;
    package_name: string;
}

/** ACP 环境状态 */
export type AcpEnvironmentStatus =
    | "checking"           // 正在检测
    | "bun-not-installed"  // Bun 未安装
    | "not-installed"      // ACP 库未安装
    | "installing"         // 正在安装
    | "updating"           // 正在更新
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
    const [isCheckingUpdate, setIsCheckingUpdate] = useState(false);
    const [latestVersion, setLatestVersion] = useState<string | null>(null);
    const [hasCheckedUpdate, setHasCheckedUpdate] = useState(false);
    const [checkUpdateError, setCheckUpdateError] = useState<string | null>(null);
    const [updateError, setUpdateError] = useState<string | null>(null);
    const [canRetryCheckWithProxy, setCanRetryCheckWithProxy] = useState(false);
    const [canRetryUpdateWithProxy, setCanRetryUpdateWithProxy] = useState(false);
    const refreshTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

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
            return null;
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
            return info;
        } catch (err) {
            console.error("检查 ACP 库失败:", err);
            setStatus("not-installed");
            return null;
        }
    }, [cliCommand, checkBunVersion]);

    const refreshAcpLibraryAfterSuccess = useCallback((action: "install" | "update") => {
        if (refreshTimerRef.current) {
            clearTimeout(refreshTimerRef.current);
        }

        setInstallLog("");
        if (action === "update" && latestVersion) {
            setLibraryInfo((prev) =>
                prev
                    ? {
                        ...prev,
                        version: latestVersion,
                    }
                    : prev,
            );
            setStatus("installed");
        } else {
            setStatus("checking");
        }

        void checkAcpLibrary();
        refreshTimerRef.current = setTimeout(() => {
            void checkAcpLibrary();
            refreshTimerRef.current = null;
        }, 600);
    }, [checkAcpLibrary, latestVersion]);

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

    const checkAcpLibraryUpdate = useCallback(async (useProxy = false) => {
        if (!cliCommand) return;

        setIsCheckingUpdate(true);
        setCheckUpdateError(null);
        if (useProxy) {
            setCanRetryCheckWithProxy(false);
        }

        try {
            const version = await invoke<string | null>("check_acp_library_update", {
                cliCommand,
                useProxy,
            });
            setLatestVersion(version);
            setHasCheckedUpdate(true);
            setCanRetryCheckWithProxy(false);

            if (version) {
                toast.success(`发现新版本: ${version}`);
            } else {
                toast.info("当前 ACP 库已是最新版本");
            }
        } catch (err) {
            console.error("检查 ACP 库更新失败:", err);
            const message = String(err);
            setCheckUpdateError(`${useProxy ? "代理检查失败" : "直连检查失败"}: ${message}`);
            if (!useProxy) {
                setCanRetryCheckWithProxy(true);
            }
            toast.error(`检查更新失败: ${err}`);
        } finally {
            setIsCheckingUpdate(false);
        }
    }, [cliCommand]);

    const updateAcpLibrary = useCallback(async (useProxy = false) => {
        if (!cliCommand) return;

        setStatus("updating");
        setInstallLog("开始更新...");
        setUpdateError(null);
        if (useProxy) {
            setCanRetryUpdateWithProxy(false);
        }

        try {
            await invoke("update_acp_library", { cliCommand, useProxy });
        } catch (err) {
            console.error("更新 ACP 库失败:", err);
            const message = String(err);
            setUpdateError(`${useProxy ? "代理更新失败" : "直连更新失败"}: ${message}`);
            if (!useProxy) {
                setCanRetryUpdateWithProxy(true);
            }
            toast.error(`更新失败: ${err}`);
            setStatus(libraryInfo?.installed ? "installed" : "not-installed");
        }
    }, [cliCommand, libraryInfo?.installed]);

    // 监听安装事件
    useEffect(() => {
        const unlistenLog = listen<string>("acp-install-log", (event) => {
            setInstallLog((prev) => prev + "\n" + event.payload);
        });

        const unlistenFinished = listen<AcpInstallFinishedPayload>(
            "acp-install-finished",
            (event) => {
                if (event.payload.cli_command === cliCommand) {
                    const action = event.payload.action ?? "install";
                    const actionLabel = action === "update" ? "更新" : "安装";
                    if (event.payload.success) {
                        toast.success(`${event.payload.package_name} ${actionLabel}成功`);
                        setLatestVersion(null);
                        setHasCheckedUpdate(false);
                        setCheckUpdateError(null);
                        setUpdateError(null);
                        setCanRetryCheckWithProxy(false);
                        setCanRetryUpdateWithProxy(false);
                        refreshAcpLibraryAfterSuccess(action);
                    } else {
                        toast.error(`${event.payload.package_name} ${actionLabel}失败`);
                        if (action === "update") {
                            setUpdateError(
                                `${event.payload.used_proxy ? "代理更新失败" : "直连更新失败"}: ${event.payload.package_name} 更新失败`,
                            );
                            if (!event.payload.used_proxy) {
                                setCanRetryUpdateWithProxy(true);
                            }
                        }
                        setStatus(action === "update" && libraryInfo?.installed ? "installed" : "not-installed");
                    }
                }
            }
        );

        return () => {
            unlistenLog.then((f) => f());
            unlistenFinished.then((f) => f());
        };
    }, [cliCommand, checkAcpLibrary, libraryInfo?.installed, refreshAcpLibraryAfterSuccess]);

    // CLI 命令变化时重新检查
    useEffect(() => {
        setLatestVersion(null);
        setHasCheckedUpdate(false);
        setCheckUpdateError(null);
        setUpdateError(null);
        setCanRetryCheckWithProxy(false);
        setCanRetryUpdateWithProxy(false);
        setInstallLog("");
        if (cliCommand) {
            void checkAcpLibrary();
        }
    }, [cliCommand, checkAcpLibrary]);

    useEffect(() => {
        return () => {
            if (refreshTimerRef.current) {
                clearTimeout(refreshTimerRef.current);
            }
        };
    }, []);

    return {
        /** 当前状态 */
        status,
        /** 库信息 */
        libraryInfo,
        /** 安装日志 */
        installLog,
        /** Bun 版本 */
        bunVersion,
        /** 是否正在检查更新 */
        isCheckingUpdate,
        /** 最新版本（有更新时） */
        latestVersion,
        /** 是否已执行过更新检查 */
        hasCheckedUpdate,
        /** 检查更新错误 */
        checkUpdateError,
        /** 更新错误 */
        updateError,
        /** 是否可使用代理重试检查更新 */
        canRetryCheckWithProxy,
        /** 是否可使用代理重试更新 */
        canRetryUpdateWithProxy,
        /** 重新检查环境 */
        checkAcpLibrary,
        /** 检查 ACP 库更新 */
        checkAcpLibraryUpdate,
        /** 安装 ACP 库 */
        installAcpLibrary,
        /** 更新 ACP 库 */
        updateAcpLibrary,
    };
};
