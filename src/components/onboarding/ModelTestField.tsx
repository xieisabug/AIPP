import { useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "@/components/ui/button";
import { detail, local, roleKey, type ProbeResult, type Provider, type Role } from "./onboardingService";

type TestState = { phase: "idle" | "running" | "success" } | { phase: "error"; message: string };

function ModelTest({ label, selected, provider }: Omit<FieldProps, "children">) {
    const [state, setState] = useState<TestState>({ phase: "idle" });

    async function test() {
        if (!provider || !selected?.code || state.phase === "running") return;
        setState({ phase: "running" });
        const args = { providerId: provider.id, modelCode: selected.code };
        try {
            if (local(provider.api_type)) {
                await invoke("test_onboarding_agent_model", args);
            } else {
                const result = await invoke<ProbeResult>("probe_chat_model", args);
                if (!result.ok) throw new Error(result.message || result.kind);
            }
            setState({ phase: "success" });
        } catch (error) {
            setState({ phase: "error", message: detail(error) });
        }
    }

    return <>
        <Button type="button" size="sm" variant="outline" className="h-9 self-end"
            aria-label={`测试${label}`} disabled={!provider || !selected?.code || state.phase === "running"}
            onClick={() => void test()}>{state.phase === "running" ? "测试中…" : "测试"}</Button>
        {state.phase === "success" && <p role="status" className="col-span-2 text-xs text-muted-foreground">测试成功</p>}
        {state.phase === "error" && <p role="alert" className="col-span-2 wrap-anywhere text-xs text-destructive">测试失败：{state.message}</p>}
    </>;
}

interface FieldProps {
    label: string;
    selected?: Role | null;
    provider?: Provider;
    children: ReactNode;
}

export function ModelTestField({ children, ...props }: FieldProps) {
    return <div className="grid min-w-0 grid-cols-[minmax(0,1fr)_auto] gap-x-2 gap-y-1.5">
        {children}
        <ModelTest key={roleKey(props.selected)} {...props} />
    </div>;
}
