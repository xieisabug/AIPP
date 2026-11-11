import { useCallback, useEffect, useRef, useState } from "react";
import { Channel, invoke } from "@tauri-apps/api/core";

/** One Node.js / agent program / SDK location, as the session would launch it. */
export interface DetectedProgram {
    path: string;
    source: string;
    version: string | null;
    /** Ran successfully (or the SDK package exists). */
    ok: boolean;
    warning: string | null;
    error: string | null;
}

export interface NativeAgentToolStatus {
    kind: "cli" | "sdk";
    display_name: string;
    default_command: string;
    configured: string;
    selected: DetectedProgram | null;
    candidates: DetectedProgram[];
    install_commands: { label: string; command: string }[];
    docs_url: string;
    can_update: boolean;
    managed_directory: string | null;
    warnings: string[];
    /** Cursor: this provider's browser sign-in (never includes the key). */
    account: CursorAccount | null;
    /** Cursor: an API key is configured and takes precedence over sign-in. */
    uses_api_key: boolean;
}

export interface CursorAccount {
    signed_in: boolean;
    email: string | null;
    expires_at_ms: number | null;
    expired: boolean;
}

export interface NativeAgentEnvironment {
    driver: string;
    node: { configured: string; required: string; selected: DetectedProgram | null; candidates: DetectedProgram[] };
    tool: NativeAgentToolStatus;
    working_directory: { configured: string; resolved: string; exists: boolean };
}

export interface NativeAgentUpdateInfo {
    latest: string;
    update_available: boolean;
    source: string;
}

/** Provider fields that change what a session launches; detection re-runs when they change. */
export const NATIVE_AGENT_DETECTION_KEYS = [
    "agent_node_command",
    "agent_cli_command",
    "agent_sdk_directory",
    "acp_working_directory",
    "acp_env_vars",
    "antigravity_runtime",
    "cursor_api_key",
] as const;

const UPDATE_CACHE_TTL_MS = 30 * 60 * 1000;
const updateCache = new Map<string, { info: NativeAgentUpdateInfo; at: number }>();

/** Clear cached latest-version lookups (tests). */
export function resetNativeAgentUpdateCache() {
    updateCache.clear();
}

/** Strip terminal colour codes and turn progress-bar carriage returns into lines. */
export function cleanUpdateLog(text: string): string {
    // eslint-disable-next-line no-control-regex
    return text.replace(/\x1b\[[0-9;?]*[ -/]*[@-~]/g, "").replace(/\r(?!\n)/g, "\n");
}

function versionOf(environment: NativeAgentEnvironment | null): string | null {
    const selected = environment?.tool.selected;
    return selected?.ok && selected.version ? selected.version : null;
}

/**
 * Detects the Node.js runtime and agent program for a native agent provider form,
 * checks the latest published release (cached per version) and runs the agent's own updater.
 * Pass an empty driver to disable (hooks cannot be called conditionally).
 */
export function useNativeAgentEnvironment(driver: string, settings: Record<string, string>, debounceMs = 400, providerId?: string) {
    const settingsKey = JSON.stringify(settings);
    const settingsRef = useRef(settings);
    settingsRef.current = settings;
    const requestRef = useRef(0);

    const [environment, setEnvironment] = useState<NativeAgentEnvironment | null>(null);
    const [detecting, setDetecting] = useState(false);
    const [detectError, setDetectError] = useState<string | null>(null);

    const [updateInfo, setUpdateInfo] = useState<NativeAgentUpdateInfo | null>(null);
    const [checkingUpdate, setCheckingUpdate] = useState(false);
    const [checkError, setCheckError] = useState<string | null>(null);

    const [updating, setUpdating] = useState(false);
    const [updateLog, setUpdateLog] = useState("");
    const [updateError, setUpdateError] = useState<string | null>(null);

    const detect = useCallback(async () => {
        if (!driver) return null;
        const request = ++requestRef.current;
        setDetecting(true);
        try {
            const result = await invoke<NativeAgentEnvironment>("detect_native_agent_environment", {
                driver,
                settings: settingsRef.current,
                providerId: providerId ? Number(providerId) : null,
            });
            if (request !== requestRef.current) return null;
            setEnvironment(result);
            setDetectError(null);
            return result;
        } catch (error) {
            if (request === requestRef.current) setDetectError(String(error));
            return null;
        } finally {
            if (request === requestRef.current) setDetecting(false);
        }
    }, [driver, providerId]);

    useEffect(() => {
        if (!driver) {
            // Disabled or switching provider: drop the previous provider's results and in-flight answers.
            requestRef.current++;
            setEnvironment(null);
            setDetectError(null);
            setDetecting(false);
            return;
        }
        const timer = setTimeout(() => void detect(), debounceMs);
        return () => clearTimeout(timer);
    }, [driver, settingsKey, detect, debounceMs]);

    const currentVersion = versionOf(environment);

    const checkUpdate = useCallback(async (options: { useProxy?: boolean; force?: boolean } = {}) => {
        if (!driver || !currentVersion) return;
        const cacheKey = `${driver}@${currentVersion}`;
        const cached = updateCache.get(cacheKey);
        if (!options.force && !options.useProxy && cached && Date.now() - cached.at < UPDATE_CACHE_TTL_MS) {
            setUpdateInfo(cached.info);
            setCheckError(null);
            return;
        }
        setCheckingUpdate(true);
        try {
            const info = await invoke<NativeAgentUpdateInfo>("check_native_agent_update", {
                driver,
                currentVersion,
                useProxy: options.useProxy ?? false,
            });
            updateCache.set(cacheKey, { info, at: Date.now() });
            setUpdateInfo(info);
            setCheckError(null);
        } catch (error) {
            setCheckError(`${options.useProxy ? "通过代理" : "直连"}检查更新失败：${String(error)}`);
        } finally {
            setCheckingUpdate(false);
        }
    }, [driver, currentVersion]);

    // T3-style: look for a newer release as soon as an installed version is known.
    useEffect(() => {
        setUpdateInfo(null);
        setCheckError(null);
        if (currentVersion && environment?.tool.can_update) void checkUpdate();
    }, [currentVersion, environment?.tool.can_update, checkUpdate]);

    /** Runs the agent's updater (or installs the Cursor SDK). Resolves to the SDK directory for Cursor. */
    const runUpdate = useCallback(async (useProxy = false): Promise<string | null> => {
        if (!driver) return null;
        setUpdating(true);
        setUpdateError(null);
        setUpdateLog("");
        const onLog = new Channel<string>();
        onLog.onmessage = (text) => setUpdateLog((previous) => (previous + cleanUpdateLog(text)).slice(-20000));
        try {
            const directory = await invoke<string | null>("run_native_agent_update", {
                driver,
                settings: settingsRef.current,
                useProxy,
                onLog,
            });
            for (const key of updateCache.keys()) if (key.startsWith(`${driver}@`)) updateCache.delete(key);
            setUpdateInfo(null);
            await detect();
            return directory ?? null;
        } catch (error) {
            setUpdateError(String(error));
            throw error;
        } finally {
            setUpdating(false);
        }
    }, [driver, detect]);

    return {
        environment,
        detecting,
        detectError,
        detect,
        currentVersion,
        updateInfo,
        checkingUpdate,
        checkError,
        checkUpdate,
        updating,
        updateLog,
        updateError,
        runUpdate,
    };
}

export type NativeAgentEnvironmentState = ReturnType<typeof useNativeAgentEnvironment>;
