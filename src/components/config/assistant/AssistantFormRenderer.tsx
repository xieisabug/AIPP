import React, { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useIsMobilePlatform } from "@/hooks/use-platform";
import { UseFormReturn } from "react-hook-form";
import ConfigForm from "@/components/ConfigForm";
import { AssistantDetail } from "@/data/Assistant";
import { AssistantFormConfig, AssistantConfigApi } from "@/types/forms";
import { Button } from "@/components/ui/button";
import { Share } from "lucide-react";
import ButlerAvatar from "@/components/butler/ButlerAvatar";
import { appearanceKey, defaultAppearance, parseAppearance } from "@/components/butler/butlerAppearance";
import { useFeatureConfig } from "@/hooks/feature/useFeatureConfig";
import { ASSISTANT_ADVANCED_FIELD_KEYS } from "@/utils/assistantFormSections";

function AssistantAppearance({ id, onEdit }: { id: number; onEdit?: () => void }) {
    const { featureConfig } = useFeatureConfig();
    const appearance = parseAppearance(featureConfig.get("butler_appearance")?.get(appearanceKey(id))) ?? defaultAppearance;
    return <button type="button" onClick={onEdit} disabled={!onEdit}
        aria-label="编辑助手形象" title="编辑名字、描述与形象"
        className="inline-flex rounded-xl p-3 hover:bg-muted focus-visible:outline-2 focus-visible:outline-ring">
        <ButlerAvatar id={id} size={96} appearance={appearance} />
    </button>;
}


interface AssistantFormRendererProps {
    currentAssistant: AssistantDetail | null;
    formConfig: AssistantFormConfig[];
    form: UseFormReturn<any>;
    /** Value-based dirty state from AssistantConfig (overrides RHF formState.isDirty). */
    isDirty?: boolean;
    isSaving?: boolean;
    isLoading?: boolean;
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
    isDirty,
    isSaving,
    isLoading,
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
    if (isLoading) return <p role="status">正在加载助手配置…</p>;

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
            advancedFieldKeys={ASSISTANT_ADVANCED_FIELD_KEYS.filter(key => !formConfig.some(item => item.key === key && item.section === "extension"))}
            extensionFieldKeys={formConfig.filter(item => item.section === "extension").map(item => item.key)}
            advancedConfigKey={assistantId}
            classNames="bottom-space"
            assistantType={currentAssistant.assistant.assistant_type}
            onSave={onSave}
            showHeaderSave={mobile}
            onCopy={currentAssistant.assistant.id === 1 ? undefined : onCopy}
            onDelete={currentAssistant.assistant.id === 1 ? undefined : onDelete}
            onTitleClick={onEdit}
            onDescriptionClick={onEdit}
            beforeFields={<AssistantAppearance id={currentAssistant.assistant.id} onEdit={onEdit} />}
            useFormReturn={form}
            isDirty={isDirty}
            isSaving={isSaving}
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
