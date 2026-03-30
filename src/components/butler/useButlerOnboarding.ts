import { useState, useCallback, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";

export interface TrustedWorkspace {
    path: string;
    description: string;
}

export interface OnboardingState {
    currentStep: number;
    totalSteps: number;
    // Step 1
    modelId: string;
    displayName: string;
    // Step 2
    bunVersion: string | null;
    uvVersion: string | null;
    bunInstalling: boolean;
    uvInstalling: boolean;
    bunInstallLog: string;
    uvInstallLog: string;
    // Step 3
    trustedWorkspaces: TrustedWorkspace[];
    trustAllWorkspaces: boolean;
    // Step 4
    feishuEnabled: boolean;
    feishuAppId: string;
    feishuAppSecret: string;
    feishuBaseUrl: string;
}

const TOTAL_STEPS = 4;

const initialState: OnboardingState = {
    currentStep: 0,
    totalSteps: TOTAL_STEPS,
    modelId: "",
    displayName: "总管家",
    bunVersion: null,
    uvVersion: null,
    bunInstalling: false,
    uvInstalling: false,
    bunInstallLog: "",
    uvInstallLog: "",
    trustedWorkspaces: [],
    trustAllWorkspaces: false,
    feishuEnabled: false,
    feishuAppId: "",
    feishuAppSecret: "",
    feishuBaseUrl: "https://open.feishu.cn",
};

interface UseButlerOnboardingOptions {
    isOpen?: boolean;
    existingModelId?: string;
    existingDisplayName?: string;
    existingTrustAll?: boolean;
    existingTrustedWorkspaces?: TrustedWorkspace[];
    existingFeishuEnabled?: boolean;
    existingFeishuAppId?: string;
    existingFeishuBaseUrl?: string;
}

export function useButlerOnboarding(options: UseButlerOnboardingOptions = {}) {
    const buildStateFromOptions = useCallback((): OnboardingState => ({
        ...initialState,
        modelId: options.existingModelId || "",
        displayName: options.existingDisplayName || "总管家",
        trustAllWorkspaces: options.existingTrustAll || false,
        trustedWorkspaces: options.existingTrustedWorkspaces || [],
        feishuEnabled: options.existingFeishuEnabled || false,
        feishuAppId: options.existingFeishuAppId || "",
        feishuBaseUrl: options.existingFeishuBaseUrl || "https://open.feishu.cn",
    }), [
        options.existingDisplayName,
        options.existingFeishuAppId,
        options.existingFeishuBaseUrl,
        options.existingFeishuEnabled,
        options.existingModelId,
        options.existingTrustAll,
        options.existingTrustedWorkspaces,
    ]);
    const [state, setState] = useState<OnboardingState>(buildStateFromOptions);

    useEffect(() => {
        if (!options.isOpen) {
            return;
        }
        setState(buildStateFromOptions());
    }, [buildStateFromOptions, options.isOpen]);

    // Step 1 setters
    const setModelId = useCallback((modelId: string) => {
        setState((prev) => ({ ...prev, modelId }));
    }, []);

    const setDisplayName = useCallback((displayName: string) => {
        setState((prev) => ({ ...prev, displayName }));
    }, []);

    // Step 2: Environment detection
    const checkBunVersion = useCallback(async () => {
        try {
            const version = await invoke<string>("check_bun_version");
            const installed = version !== "Not Installed" && version !== "";
            setState((prev) => ({ ...prev, bunVersion: installed ? version : null }));
            return installed;
        } catch {
            setState((prev) => ({ ...prev, bunVersion: null }));
            return false;
        }
    }, []);

    const checkUvVersion = useCallback(async () => {
        try {
            const version = await invoke<string>("check_uv_version");
            const installed = version !== "Not Installed" && version !== "";
            setState((prev) => ({ ...prev, uvVersion: installed ? version : null }));
            return installed;
        } catch {
            setState((prev) => ({ ...prev, uvVersion: null }));
            return false;
        }
    }, []);

    const installBun = useCallback(async () => {
        setState((prev) => ({ ...prev, bunInstalling: true, bunInstallLog: "开始安装 Bun..." }));
        try {
            await invoke("install_bun", { targetWindow: null });
        } catch (err) {
            toast.error(`安装 Bun 失败: ${err}`);
            setState((prev) => ({ ...prev, bunInstalling: false }));
        }
    }, []);

    const installUv = useCallback(async () => {
        setState((prev) => ({ ...prev, uvInstalling: true, uvInstallLog: "开始安装 uv..." }));
        try {
            await invoke("install_uv", { targetWindow: null });
        } catch (err) {
            toast.error(`安装 uv 失败: ${err}`);
            setState((prev) => ({ ...prev, uvInstalling: false }));
        }
    }, []);

    // Listen to install events
    useEffect(() => {
        const unlistenBunLog = listen<string>("bun-install-log", (event) => {
            setState((prev) => ({
                ...prev,
                bunInstallLog: prev.bunInstallLog + "\n" + event.payload,
            }));
        });
        const unlistenBunFinished = listen<boolean>("bun-install-finished", (event) => {
            setState((prev) => ({ ...prev, bunInstalling: false }));
            if (event.payload) {
                toast.success("Bun 安装成功");
                void checkBunVersion();
            } else {
                toast.error("Bun 安装失败");
            }
        });
        const unlistenUvLog = listen<string>("uv-install-log", (event) => {
            setState((prev) => ({
                ...prev,
                uvInstallLog: prev.uvInstallLog + "\n" + event.payload,
            }));
        });
        const unlistenUvFinished = listen<boolean>("uv-install-finished", (event) => {
            setState((prev) => ({ ...prev, uvInstalling: false }));
            if (event.payload) {
                toast.success("uv 安装成功");
                void checkUvVersion();
            } else {
                toast.error("uv 安装失败");
            }
        });

        return () => {
            unlistenBunLog.then((f) => f());
            unlistenBunFinished.then((f) => f());
            unlistenUvLog.then((f) => f());
            unlistenUvFinished.then((f) => f());
        };
    }, [checkBunVersion, checkUvVersion]);

    // Step 3 setters
    const setTrustAllWorkspaces = useCallback((trustAll: boolean) => {
        setState((prev) => ({ ...prev, trustAllWorkspaces: trustAll }));
    }, []);

    const addTrustedWorkspace = useCallback((path: string, description: string) => {
        setState((prev) => {
            if (prev.trustedWorkspaces.some((ws) => ws.path === path)) {
                return prev;
            }
            return {
                ...prev,
                trustedWorkspaces: [...prev.trustedWorkspaces, { path, description }],
            };
        });
    }, []);

    const removeTrustedWorkspace = useCallback((path: string) => {
        setState((prev) => ({
            ...prev,
            trustedWorkspaces: prev.trustedWorkspaces.filter((ws) => ws.path !== path),
        }));
    }, []);

    const updateWorkspaceDescription = useCallback((path: string, description: string) => {
        setState((prev) => ({
            ...prev,
            trustedWorkspaces: prev.trustedWorkspaces.map((ws) =>
                ws.path === path ? { ...ws, description } : ws
            ),
        }));
    }, []);

    // Step 4 setters
    const setFeishuEnabled = useCallback((enabled: boolean) => {
        setState((prev) => ({ ...prev, feishuEnabled: enabled }));
    }, []);

    const setFeishuAppId = useCallback((appId: string) => {
        setState((prev) => ({ ...prev, feishuAppId: appId }));
    }, []);

    const setFeishuAppSecret = useCallback((secret: string) => {
        setState((prev) => ({ ...prev, feishuAppSecret: secret }));
    }, []);

    const setFeishuBaseUrl = useCallback((url: string) => {
        setState((prev) => ({ ...prev, feishuBaseUrl: url }));
    }, []);

    // Navigation
    const goNext = useCallback(() => {
        setState((prev) => ({
            ...prev,
            currentStep: Math.min(prev.currentStep + 1, TOTAL_STEPS - 1),
        }));
    }, []);

    const goPrev = useCallback(() => {
        setState((prev) => ({
            ...prev,
            currentStep: Math.max(prev.currentStep - 1, 0),
        }));
    }, []);

    const goToStep = useCallback((step: number) => {
        setState((prev) => ({
            ...prev,
            currentStep: Math.max(0, Math.min(step, TOTAL_STEPS - 1)),
        }));
    }, []);

    const canGoNext = state.currentStep < TOTAL_STEPS - 1;
    const canGoPrev = state.currentStep > 0;
    const isLastStep = state.currentStep === TOTAL_STEPS - 1;

    // Step 1 requires model selection
    const isStepValid = useCallback((step: number): boolean => {
        switch (step) {
            case 0:
                return !!state.modelId;
            case 1:
            case 2:
            case 3:
                return true;
            default:
                return false;
        }
    }, [state.modelId]);

    return {
        state,
        // Step 1
        setModelId,
        setDisplayName,
        // Step 2
        checkBunVersion,
        checkUvVersion,
        installBun,
        installUv,
        // Step 3
        setTrustAllWorkspaces,
        addTrustedWorkspace,
        removeTrustedWorkspace,
        updateWorkspaceDescription,
        // Step 4
        setFeishuEnabled,
        setFeishuAppId,
        setFeishuAppSecret,
        setFeishuBaseUrl,
        // Navigation
        goNext,
        goPrev,
        goToStep,
        canGoNext,
        canGoPrev,
        isLastStep,
        isStepValid,
    };
}
