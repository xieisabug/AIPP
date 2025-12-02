import { useState, useCallback, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";

interface CopilotAuthInfo {
    userCode?: string;
    verificationUri?: string;
    isAuthorizing: boolean;
}

interface CopilotLspStatus {
    is_running: boolean;
    is_authorized: boolean;
    user?: string;
    error?: string;
}

// 授权方式类型
export type CopilotAuthMethod = "scan_config" | "oauth_flow" | "manual_token";

interface UseCopilotProps {
    llmProviderId: string;
    onAuthSuccess?: () => void;
}

interface UseCopilotReturn {
    authInfo: CopilotAuthInfo;
    isAuthorizing: boolean;
    lspStatus: CopilotLspStatus | null;
    startAuthorization: () => Promise<void>;
    cancelAuthorization: () => Promise<void>;
    useLspAuth: boolean;
    setUseLspAuth: (value: boolean) => void;
    // 新增：三种授权方式
    scanConfigAuth: () => Promise<void>;
    oauthFlowAuth: () => Promise<void>;
    manualTokenAuth: (token: string) => Promise<void>;
}

export const useCopilot = ({
    llmProviderId,
    onAuthSuccess,
}: UseCopilotProps): UseCopilotReturn => {
    const [authInfo, setAuthInfo] = useState<CopilotAuthInfo>({
        isAuthorizing: false,
    });
    const [lspStatus, setLspStatus] = useState<CopilotLspStatus | null>(null);
    const [useLspAuth, setUseLspAuth] = useState<boolean>(true); // 默认使用 LSP 认证

    // 检查 LSP 状态
    const checkLspStatus = useCallback(async () => {
        try {
            const status = await invoke<CopilotLspStatus>("get_copilot_lsp_status");
            setLspStatus(status);
            return status;
        } catch (e) {
            console.error("[Copilot] Failed to get LSP status", e);
            return null;
        }
    }, []);

    // 组件挂载时检查 LSP 状态
    useEffect(() => {
        checkLspStatus();
    }, [checkLspStatus]);

    // 方式1: 扫描 apps.json 配置文件
    const scanConfigAuth = useCallback(async () => {
        try {
            setAuthInfo({ isAuthorizing: true });

            // 尝试从 apps.json 读取已有的 OAuth token
            const existingToken = await invoke<string | null>("get_copilot_oauth_token_from_config");
            if (existingToken) {
                console.info("[Copilot] Found existing OAuth token in apps.json");
                
                // 保存 OAuth token 到 api_key
                await invoke("update_llm_provider_config", {
                    llmProviderId: llmProviderId,
                    name: "api_key",
                    value: existingToken,
                });

                toast.success("使用已有的 Copilot 授权配置成功！");
                setAuthInfo({ isAuthorizing: false });
                onAuthSuccess?.();
            } else {
                toast.error("未在 apps.json 中找到已有的授权配置，请尝试其他授权方式");
                setAuthInfo({ isAuthorizing: false });
            }
        } catch (e) {
            console.error("[Copilot] Scan config failed", e);
            toast.error("扫描配置失败: " + e);
            setAuthInfo({ isAuthorizing: false });
        }
    }, [llmProviderId, onAuthSuccess]);

    // 方式2: 使用 OAuth Device Flow 进行授权（直接调用 GitHub API，不需要 LSP）
    const oauthFlowAuth = useCallback(async () => {
        try {
            setAuthInfo({ isAuthorizing: true });

            // 启动 device flow（直接调用 GitHub API）
            toast.info("正在启动 GitHub 授权流程...");
            const startResp = await invoke<{
                device_code: string;
                user_code: string;
                verification_uri: string;
                expires_in: number;
                interval: number;
            }>("start_github_copilot_device_flow", {
                llmProviderId: parseInt(llmProviderId, 10),
            });

            console.info("[Copilot] Device flow started", startResp);

            // 显示授权码
            setAuthInfo({
                userCode: startResp.user_code,
                verificationUri: startResp.verification_uri,
                isAuthorizing: true,
            });

            toast.info(
                `浏览器将自动打开授权页面，请输入授权码: ${startResp.user_code}`,
                { duration: 10000 }
            );

            // 轮询授权结果（OAuth token 会被自动保存到 api_key）
            const authResult = await invoke<{
                access_token: string;
                token_type: string;
            }>("poll_github_copilot_token", {
                llmProviderId: parseInt(llmProviderId, 10),
                deviceCode: startResp.device_code,
                interval: startResp.interval,
            });

            console.info("[Copilot] Device flow authorized", authResult);
            toast.success("GitHub Copilot 配置完成，可以开始使用了！");

            setAuthInfo({ isAuthorizing: false });
            onAuthSuccess?.();
        } catch (e) {
            console.error("[Copilot] OAuth flow failed", e);
            toast.error("OAuth 授权失败: " + e);
            setAuthInfo({ isAuthorizing: false });
        }
    }, [llmProviderId, onAuthSuccess]);

    // 方式3: 手动输入 OAuth Token
    const manualTokenAuth = useCallback(async (token: string) => {
        try {
            setAuthInfo({ isAuthorizing: true });

            if (!token || token.trim().length === 0) {
                toast.error("请输入有效的 OAuth Token");
                setAuthInfo({ isAuthorizing: false });
                return;
            }

            const trimmedToken = token.trim();

            // 保存 OAuth token 到 api_key
            await invoke("update_llm_provider_config", {
                llmProviderId: llmProviderId,
                name: "api_key",
                value: trimmedToken,
            });

            toast.success("OAuth Token 已保存！");
            setAuthInfo({ isAuthorizing: false });
            onAuthSuccess?.();
        } catch (e) {
            console.error("[Copilot] Manual token auth failed", e);
            toast.error("保存 Token 失败: " + e);
            setAuthInfo({ isAuthorizing: false });
        }
    }, [llmProviderId, onAuthSuccess]);

    // 保留原有的 startLspAuthorization 方法用于兼容（现在使用直接的 Device Flow）
    const startLspAuthorization = useCallback(async () => {
        await oauthFlowAuth();
    }, [oauthFlowAuth]);

    const startAuthorization = useCallback(async () => {
        // 统一使用 Device Flow 方式
        await oauthFlowAuth();
    }, [oauthFlowAuth]);

    const cancelAuthorization = useCallback(async () => {
        try {
            // 停止 LSP 服务器
            try {
                await invoke("stop_copilot_lsp");
            } catch (e) {
                console.warn("[Copilot] Failed to stop LSP server", e);
            }

            await invoke("update_llm_provider_config", {
                llmProviderId: llmProviderId,
                name: "api_key",
                value: "",
            });

            toast.success("已取消 GitHub Copilot 授权");

            // 调用成功回调以刷新状态
            onAuthSuccess?.();
        } catch (e) {
            console.error("[Copilot] Cancel authorization failed", e);
            toast.error("取消授权失败: " + e);
        }
    }, [llmProviderId, onAuthSuccess]);

    return {
        authInfo,
        isAuthorizing: authInfo.isAuthorizing,
        lspStatus,
        startAuthorization,
        cancelAuthorization,
        useLspAuth,
        setUseLspAuth,
        // 新增：三种授权方式
        scanConfigAuth,
        oauthFlowAuth,
        manualTokenAuth,
    };
};
