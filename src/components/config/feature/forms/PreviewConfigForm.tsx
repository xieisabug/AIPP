import React from "react";
import { UseFormReturn } from "react-hook-form";
import ConfigForm from "@/components/ConfigForm";

interface PreviewConfigFormProps {
    form: UseFormReturn<any>;
    bunVersion: string;
    uvVersion: string;
    isInstallingBun: boolean;
    isInstallingUv: boolean;
    bunInstallLog: string;
    uvInstallLog: string;
    onInstallBun: () => void;
    onInstallUv: () => void;
    bunLatestVersion: string | null;
    uvLatestVersion: string | null;
    isCheckingBunUpdate: boolean;
    isCheckingUvUpdate: boolean;
    isUpdatingBun: boolean;
    isUpdatingUv: boolean;
    checkBunUpdate: (useProxy: boolean) => void;
    checkUvUpdate: (useProxy: boolean) => void;
    updateBun: (useProxy: boolean) => void;
    updateUv: (useProxy: boolean) => void;
    // Python 相关
    python2Version: string;
    python3Version: string;
    installedPythons: string[];
    needInstallPython3: boolean;
    isInstallingPython: boolean;
    pythonInstallLog: string;
    checkPythonVersions: () => void;
    installPython3: () => void;
}

export const PreviewConfigForm: React.FC<PreviewConfigFormProps> = ({
    form,
    bunVersion,
    uvVersion,
    isInstallingBun,
    isInstallingUv,
    bunInstallLog,
    uvInstallLog,
    onInstallBun,
    onInstallUv,
    bunLatestVersion,
    uvLatestVersion,
    isCheckingBunUpdate,
    isCheckingUvUpdate,
    isUpdatingBun,
    isUpdatingUv,
    checkBunUpdate,
    checkUvUpdate,
    updateBun,
    updateUv,
    // Python 相关
    python2Version,
    python3Version,
    installedPythons,
    needInstallPython3,
    isInstallingPython,
    pythonInstallLog,
    checkPythonVersions,
    installPython3,
}) => {
    const bunNotInstalled = bunVersion === "Not Installed";
    const uvNotInstalled = uvVersion === "Not Installed";

    const PREVIEW_FORM_CONFIG: Array<{ key: string; config: any }> = [];

    // Bun 配置
    if (bunNotInstalled) {
        PREVIEW_FORM_CONFIG.push({
            key: "bun_install",
            config: {
                type: "button" as const,
                label: "安装 Bun",
                value: isInstallingBun ? "安装中..." : "安装",
                onClick: onInstallBun,
                disabled: isInstallingBun,
            },
        });
    } else {
        PREVIEW_FORM_CONFIG.push({
            key: "bun_version",
            config: {
                type: "inline-buttons" as const,
                label: "Bun 版本",
                value: bunVersion,
                buttons: [
                    {
                        text: "检查更新",
                        onClick: () => checkBunUpdate(false),
                        disabled: isCheckingBunUpdate || isUpdatingBun,
                        variant: "outline" as const,
                    },
                    {
                        text: "使用代理检查更新",
                        onClick: () => checkBunUpdate(true),
                        disabled: isCheckingBunUpdate || isUpdatingBun,
                        variant: "outline" as const,
                    },
                ],
            },
        });
    }

    // Bun 安装日志
    PREVIEW_FORM_CONFIG.push({
        key: "bun_log",
        config: {
            type: "static" as const,
            label: "Bun 安装日志",
            value: bunInstallLog || "",
            hidden: !isInstallingBun && !isUpdatingBun,
        },
    });

    // Bun 更新按钮（如果有新版本）
    if (bunLatestVersion && !bunNotInstalled) {
        PREVIEW_FORM_CONFIG.push({
            key: "bun_update",
            config: {
                type: "button" as const,
                label: "Bun 更新",
                value: isUpdatingBun ? "更新中..." : `更新到 ${bunLatestVersion}`,
                onClick: () => updateBun(false),
                disabled: isUpdatingBun,
            },
        });
        PREVIEW_FORM_CONFIG.push({
            key: "bun_update_proxy",
            config: {
                type: "button" as const,
                label: "Bun 使用代理更新",
                value: isUpdatingBun ? "更新中..." : `使用代理更新到 ${bunLatestVersion}`,
                onClick: () => updateBun(true),
                disabled: isUpdatingBun,
                className: "border-orange-500/50 text-orange-600",
            },
        });
    }

    // UV 配置
    if (uvNotInstalled) {
        PREVIEW_FORM_CONFIG.push({
            key: "uv_install",
            config: {
                type: "button" as const,
                label: "安装 UV",
                value: isInstallingUv ? "安装中..." : "安装",
                onClick: onInstallUv,
                disabled: isInstallingUv,
            },
        });
    } else {
        PREVIEW_FORM_CONFIG.push({
            key: "uv_version",
            config: {
                type: "inline-buttons" as const,
                label: "UV 版本",
                value: uvVersion,
                buttons: [
                    {
                        text: "检查更新",
                        onClick: () => checkUvUpdate(false),
                        disabled: isCheckingUvUpdate || isUpdatingUv,
                        variant: "outline" as const,
                    },
                    {
                        text: "使用代理检查更新",
                        onClick: () => checkUvUpdate(true),
                        disabled: isCheckingUvUpdate || isUpdatingUv,
                        variant: "outline" as const,
                    },
                ],
            },
        });
    }

    // UV 安装日志
    PREVIEW_FORM_CONFIG.push({
        key: "uv_log",
        config: {
            type: "static" as const,
            label: "UV 安装日志",
            value: uvInstallLog || "",
            hidden: !isInstallingUv && !isUpdatingUv,
        },
    });

    // UV 更新按钮（如果有新版本）
    if (uvLatestVersion && !uvNotInstalled) {
        PREVIEW_FORM_CONFIG.push({
            key: "uv_update",
            config: {
                type: "button" as const,
                label: "UV 更新",
                value: isUpdatingUv ? "更新中..." : `更新到 ${uvLatestVersion}`,
                onClick: () => updateUv(false),
                disabled: isUpdatingUv,
            },
        });
        PREVIEW_FORM_CONFIG.push({
            key: "uv_update_proxy",
            config: {
                type: "button" as const,
                label: "UV 使用代理更新",
                value: isUpdatingUv ? "更新中..." : `使用代理更新到 ${uvLatestVersion}`,
                onClick: () => updateUv(true),
                disabled: isUpdatingUv,
                className: "border-orange-500/50 text-orange-600",
            },
        });
    }

    // Python 配置 - 仅在 uv 已安装时显示
    if (uvVersion !== "Not Installed") {
        // Python 版本显示
        PREVIEW_FORM_CONFIG.push({
            key: "python_versions",
            config: {
                type: "inline-buttons" as const,
                label: "Python 版本",
                value: `Python 2: ${python2Version}, Python 3: ${python3Version}`,
                buttons: [
                    {
                        text: "刷新",
                        onClick: checkPythonVersions,
                        disabled: isInstallingPython,
                        variant: "outline" as const,
                    },
                ],
            },
        });

        // Python 安装日志
        PREVIEW_FORM_CONFIG.push({
            key: "python_log",
            config: {
                type: "static" as const,
                label: "Python 安装日志",
                value: pythonInstallLog || "",
                hidden: !isInstallingPython,
            },
        });

        // Python 安装按钮（如果需要安装 Python 3）
        if (needInstallPython3) {
            const installMessage = python2Version !== "Not Installed"
                ? "检测到 Python 2，建议安装 Python 3"
                : "未检测到 Python，建议安装 Python 3";

            PREVIEW_FORM_CONFIG.push({
                key: "python_install_message",
                config: {
                    type: "static" as const,
                    label: "提示",
                    value: installMessage,
                },
            });

            PREVIEW_FORM_CONFIG.push({
                key: "python_install",
                config: {
                    type: "button" as const,
                    label: "安装 Python 3",
                    value: isInstallingPython ? "安装中..." : "安装",
                    onClick: installPython3,
                    disabled: isInstallingPython,
                },
            });
        }

        // 显示 uv 管理的 Python 版本列表
        if (installedPythons.length > 0) {
            PREVIEW_FORM_CONFIG.push({
                key: "installed_pythons",
                config: {
                    type: "static" as const,
                    label: "uv 管理的 Python",
                    value: installedPythons.join(", "),
                },
            });
        }
    }

    return (
        <ConfigForm
            title="预览配置"
            description="在大模型编写完react或者vue组件之后，能够快速预览"
            config={PREVIEW_FORM_CONFIG}
            layout="default"
            classNames="bottom-space"
            useFormReturn={form}
        />
    );
};
