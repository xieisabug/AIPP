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

interface SignInInitiateResult {
    status: "AlreadySignedIn" | "PromptUserDeviceFlow";
    user?: string;
    user_code?: string;
    verification_uri?: string;
}

interface SignInStatus {
    status: "OK" | "AlreadySignedIn" | "MaybeOk" | "NotAuthorized" | "NotSignedIn";
    user?: string;
}

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

    // 使用 LSP 进行授权
    const startLspAuthorization = useCallback(async () => {
        try {
            setAuthInfo({ isAuthorizing: true });

            // 1. 先尝试从 apps.json 读取已有的 OAuth token
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
                return;
            }

            // 2. 启动 LSP
            toast.info("正在启动 Copilot Language Server...");
            await invoke("start_copilot_lsp");

            // 3. 发起登录
            const signInResult = await invoke<SignInInitiateResult>("sign_in_initiate");
            console.info("[Copilot] Sign in initiate result", signInResult);

            if (signInResult.status === "AlreadySignedIn") {
                // 已经登录，获取 OAuth token
                toast.success(`已使用 ${signInResult.user} 登录 GitHub Copilot`);

                // 从 apps.json 读取 token 并保存到 api_key
                const oauthToken = await invoke<string | null>("get_copilot_oauth_token_from_config");
                if (oauthToken) {
                    await invoke("update_llm_provider_config", {
                        llmProviderId: llmProviderId,
                        name: "api_key",
                        value: oauthToken,
                    });

                    toast.success("GitHub Copilot 配置完成！");
                }

                setAuthInfo({ isAuthorizing: false });
                onAuthSuccess?.();
                return;
            }

            // 4. 需要 Device Flow 授权
            if (signInResult.status === "PromptUserDeviceFlow" && signInResult.user_code) {
                setAuthInfo({
                    userCode: signInResult.user_code,
                    verificationUri: signInResult.verification_uri,
                    isAuthorizing: true,
                });

                toast.info(
                    `浏览器将自动打开授权页面，请输入授权码: ${signInResult.user_code}`,
                    { duration: 10000 }
                );

                // 5. 等待用户完成授权并确认
                const confirmResult = await invoke<SignInStatus>("sign_in_confirm", {
                    userCode: signInResult.user_code,
                });

                console.info("[Copilot] Sign in confirm result", confirmResult);

                if (confirmResult.status === "OK" || confirmResult.status === "AlreadySignedIn" || confirmResult.status === "MaybeOk") {
                    toast.success("GitHub Copilot 授权成功！");

                    // 从 apps.json 读取 token 并保存到 api_key
                    const oauthToken = await invoke<string | null>("get_copilot_oauth_token_from_config");
                    if (oauthToken) {
                        await invoke("update_llm_provider_config", {
                            llmProviderId: llmProviderId,
                            name: "api_key",
                            value: oauthToken,
                        });

                        toast.success("GitHub Copilot 配置完成！");
                    }

                    setAuthInfo({ isAuthorizing: false });
                    onAuthSuccess?.();
                } else {
                    toast.error("GitHub Copilot 授权失败: " + confirmResult.status);
                    setAuthInfo({ isAuthorizing: false });
                }
            }
        } catch (e) {
            console.error("[Copilot] LSP authorization failed", e);
            toast.error("GitHub Copilot 授权失败: " + e);
            setAuthInfo({ isAuthorizing: false });
        }
    }, [llmProviderId, onAuthSuccess]);

    // 使用 Device Flow 进行授权（传统方式）
    const startDeviceFlowAuthorization = useCallback(async () => {
        try {
            setAuthInfo({ isAuthorizing: true });

            // 1. 启动 device flow
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
                { duration: 8000 }
            );

            // 2. 轮询授权结果（OAuth token 会被自动保存到 api_key）
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

            // 调用成功回调
            onAuthSuccess?.();
        } catch (e) {
            console.error("[Copilot] Device flow failed", e);
            toast.error("GitHub Copilot 授权失败: " + e);
            setAuthInfo({ isAuthorizing: false });
        }
    }, [llmProviderId, onAuthSuccess]);

    const startAuthorization = useCallback(async () => {
        if (useLspAuth) {
            await startLspAuthorization();
        } else {
            await startDeviceFlowAuthorization();
        }
    }, [useLspAuth, startLspAuthorization, startDeviceFlowAuthorization]);

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
    };
};
