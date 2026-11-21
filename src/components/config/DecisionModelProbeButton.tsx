import React, { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { getErrorMessage } from "@/utils/error";

interface DecisionProbeResult {
    ok: boolean;
    protocol: string;
    protocol_label: string;
    endpoint_url: string;
    latency_ms: number;
    probability?: number | null;
    input_tokens?: number | null;
    message: string;
}

interface DecisionModelProbeButtonProps {
    /** 格式为 `${model_code}%%${provider_id}` */
    modelValue: string;
    disabled?: boolean;
}

export const DecisionModelProbeButton: React.FC<DecisionModelProbeButtonProps> = ({
    modelValue,
    disabled,
}) => {
    const [busy, setBusy] = useState(false);
    const [status, setStatus] = useState<{ ok: boolean; text: string } | null>(null);

    const separatorIndex = modelValue.lastIndexOf("%%");
    const modelCode = separatorIndex > 0 ? modelValue.slice(0, separatorIndex).trim() : "";
    const providerId = separatorIndex > 0 ? Number(modelValue.slice(separatorIndex + 2).trim()) : NaN;

    React.useEffect(() => {
        setStatus(null);
    }, [modelValue]);

    if (!modelCode || !Number.isFinite(providerId) || providerId <= 0) {
        return null;
    }

    const handleProbe = async () => {
        setBusy(true);
        setStatus(null);
        try {
            const result = await invoke<DecisionProbeResult>("test_decision_model", {
                providerId,
                modelCode,
            });
            setStatus({ ok: true, text: result.message });
            toast.success(result.message);
        } catch (err) {
            const message = getErrorMessage(err);
            setStatus({ ok: false, text: message });
            toast.error("决策接口测试失败：" + message);
        } finally {
            setBusy(false);
        }
    };

    return (
        <div className="space-y-1.5">
            <Button
                type="button"
                variant="outline"
                size="sm"
                disabled={disabled || busy}
                onClick={handleProbe}
            >
                {busy ? "测试中…" : "测试决策接口"}
            </Button>
            {status ? (
                <p
                    className={`text-xs ${
                        status.ok ? "text-emerald-600 dark:text-emerald-400" : "text-destructive"
                    }`}
                >
                    {status.text}
                </p>
            ) : null}
        </div>
    );
};
