import React, { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useIsMobilePlatform } from "@/hooks/use-platform";
import { UseFormReturn } from "react-hook-form";
import ConfigForm from "@/components/ConfigForm";
import { AssistantDetail } from "@/data/Assistant";
import { AssistantFormConfig, AssistantConfigApi } from "@/types/forms";
import { Button } from "@/components/ui/button";
import { Share } from "lucide-react";

interface AssistantFormRendererProps {
    currentAssistant: AssistantDetail | null;
    formConfig: AssistantFormConfig[];
    form: UseFormReturn<any>;
    assistantConfigApi: AssistantConfigApi;
    onSave: () => void;
    onCopy?: () => void;
    onDelete?: () => void;
    onEdit?: () => void;
    onShare?: () => void;
}

export const AssistantFormRenderer: React.FC<AssistantFormRendererProps> = ({
    currentAssistant,
    formConfig,
    form,
    assistantConfigApi,
    onSave,
    onCopy,
    onDelete,
    onEdit,
    onShare,
}) => {
    const mobile = useIsMobilePlatform();
    const [compatibility, setCompatibility] = useState<{ id: number; reasons: string[] } | null>(null);
    const assistantId = currentAssistant?.assistant.id;
    useEffect(() => {
        if (!mobile || assistantId == null) return;
        let active = true;
        invoke<{ available: boolean; issues: { capability: string; reason: string }[] }>("get_assistant_execution_capability", { assistantId })
            .then(result => {
                if (active) setCompatibility({ id: assistantId, reasons: result.issues.filter(issue => issue.capability === "plugin_assistant" || issue.capability === "local_cli_agent").map(issue => issue.reason) });
            })
            .catch(error => { if (active) setCompatibility({ id: assistantId, reasons: [`能力校验失败：${String(error)}`] }); });
        return () => { active = false; };
    }, [mobile, assistantId]);
    if (!currentAssistant) {
        return null;
    }

    if (mobile && compatibility?.id !== assistantId) return <p role="status">正在检查助手兼容性…</p>;
    if (mobile && compatibility?.reasons.length) return <section className="p-4 space-y-3">
        <h2>{currentAssistant.assistant.name}</h2>
        <p>{currentAssistant.assistant.description}</p>
        <p>此助手在移动端只读，原配置已保留。请在桌面端编辑。</p>
        {compatibility.reasons.map(reason => <p key={reason} role="status">{reason}</p>)}
    </section>;

    return (
        <ConfigForm
            assistantConfigApi={assistantConfigApi}
            title={currentAssistant.assistant.name}
            description={currentAssistant.assistant.description || "配置你的智能助手"}
            config={formConfig}
            layout="prompt"
            classNames="bottom-space"
            assistantType={currentAssistant.assistant.assistant_type}
            onSave={onSave}
            onCopy={currentAssistant.assistant.id === 1 ? undefined : onCopy}
            onDelete={currentAssistant.assistant.id === 1 ? undefined : onDelete}
            onEdit={onEdit}
            useFormReturn={form}
            extraButtons={
                onShare ? (
                    <div className="flex gap-2">
                        <Button
                            variant="ghost"
                            size="sm"
                            onClick={onShare}
                            className="gap-2"
                        >
                            <Share className="h-4 w-4" />
                        </Button>
                    </div>
                ) : undefined
            }
        />
    );
};
