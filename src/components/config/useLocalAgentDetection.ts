import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { AcpLibraryInfo } from "@/hooks/feature/useAcpEnvironment";
import type { NativeAgentEnvironment } from "@/hooks/feature/useNativeAgentEnvironment";
import { ACP_CLI_OPTIONS, type ProviderApiTypeOption } from "./providerApiTypes";

export type LocalAgentStatus =
    | { state: "detecting" }
    | {
        state: "installed" | "missing";
        /** 被检测的程序名，如「Codex CLI」「Cursor SDK」 */
        program: string;
        version: string | null;
        path: string | null;
        /** 原生智能体经 Node.js 桥接运行；其他类型为 undefined */
        node?: { ok: boolean; version: string | null; required: string };
    }
    | { state: "error"; message: string };

export interface AcpCliStatus {
    value: string;
    label: string;
    installed: boolean;
    version: string | null;
}

/** 通过 check_acp_library 检测的类型：命令 → 显示名 */
const CLI_CHECKS: Record<string, { command: string; program: string }> = {
    codex_app_server: { command: "codex", program: "Codex CLI" },
    claude_sdk: { command: "claude", program: "Claude Code CLI" },
};

async function detectNative(driver: string): Promise<LocalAgentStatus> {
    const env = await invoke<NativeAgentEnvironment>("detect_native_agent_environment", { driver, settings: {}, providerId: null });
    const tool = env.tool;
    const found = tool.selected?.ok ? tool.selected : tool.candidates.find((candidate) => candidate.ok) ?? null;
    const node = env.node.selected;
    return {
        state: found ? "installed" : "missing",
        program: tool.display_name,
        version: found?.version ?? null,
        path: found?.path ?? null,
        node: { ok: Boolean(node?.ok), version: node?.version ?? null, required: env.node.required },
    };
}

async function detectCli(command: string, program: string): Promise<LocalAgentStatus> {
    const info = await invoke<AcpLibraryInfo>("check_acp_library", { cliCommand: command });
    return {
        state: info.installed ? "installed" : "missing",
        program,
        version: info.version,
        path: info.installed_path,
    };
}

/**
 * 检测本机已安装的本地智能体（打开新增提供商向导时一次性并行检测）。
 * 复用详情页的检测命令，保证结论与会话实际启动的程序一致。
 */
export function useLocalAgentDetection(apiTypes: readonly ProviderApiTypeOption[]) {
    const localTypes = apiTypes.filter((type) => type.kind === "local").map((type) => type.value);
    const key = localTypes.join(",");
    const [statuses, setStatuses] = useState<Record<string, LocalAgentStatus>>({});
    const [acpClis, setAcpClis] = useState<AcpCliStatus[] | null>(null);

    useEffect(() => {
        const types = key ? key.split(",") : [];
        if (types.length === 0) return;
        let cancelled = false;
        setStatuses(Object.fromEntries(types.map((type) => [type, { state: "detecting" } as LocalAgentStatus])));
        const settle = (type: string, status: LocalAgentStatus) => {
            if (!cancelled) setStatuses((prev) => ({ ...prev, [type]: status }));
        };
        const failed = (error: unknown): LocalAgentStatus => ({ state: "error", message: String(error) });

        for (const type of types) {
            if (type === "acp") continue;
            const cli = CLI_CHECKS[type];
            const detection = cli ? detectCli(cli.command, cli.program) : detectNative(type);
            detection.then((status) => settle(type, status), (error) => settle(type, failed(error)));
        }

        if (types.includes("acp")) {
            Promise.all(ACP_CLI_OPTIONS.map(async (option) => {
                try {
                    const info = await invoke<AcpLibraryInfo>("check_acp_library", { cliCommand: option.value });
                    return { value: option.value, label: option.label, installed: info.installed, version: info.version };
                } catch {
                    return { value: option.value, label: option.label, installed: false, version: null };
                }
            })).then((clis) => {
                if (cancelled) return;
                setAcpClis(clis);
                const first = clis.find((cli) => cli.installed);
                settle("acp", {
                    state: first ? "installed" : "missing",
                    program: first ? first.value : "ACP CLI",
                    version: first?.version ?? null,
                    path: null,
                });
            });
        }
        return () => {
            cancelled = true;
        };
    }, [key]);

    return { statuses, acpClis };
}
