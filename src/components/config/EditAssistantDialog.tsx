import React from "react";
import { AssistantDetail } from "@/data/Assistant";
import { toast } from "sonner";
import ButlerProfileDialog from "@/components/butler/ButlerProfileDialog";
import { appearanceKey, parseAppearance } from "@/components/butler/butlerAppearance";
import { useFeatureConfig } from "@/hooks/feature/useFeatureConfig";
import type { FeatureConfig } from "@/hooks/feature/useFeatureConfig";
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "@/components/ui/dialog";

interface EditAssistantDialogProps {
    isOpen: boolean;
    onClose: () => void;
    currentAssistant: AssistantDetail | null;
    onSave: (assistant: AssistantDetail) => Promise<void>;
    onAssistantUpdated: (updatedAssistant: AssistantDetail) => void;
}

const EditAssistantDialog: React.FC<EditAssistantDialogProps> = ({
    isOpen, onClose, currentAssistant, onSave, onAssistantUpdated,
}) => {
    const { loadFeatureConfig, saveFeatureConfig } = useFeatureConfig();
    const [loaded, setLoaded] = React.useState<{ id: number; config: FeatureConfig } | null>(null);
    const [error, setError] = React.useState("");
    const id = currentAssistant?.assistant.id;
    React.useEffect(() => {
        if (!isOpen || id == null) { setLoaded(null); return; }
        let active = true;
        setLoaded(null);
        setError("");
        void loadFeatureConfig().then(config => {
            if (active) setLoaded({ id, config });
        }).catch(cause => {
            if (active) setError(`读取助手 ${id} 形象失败：${String(cause)}`);
        });
        return () => { active = false; };
    }, [isOpen, id, loadFeatureConfig]);
    if (!isOpen || !currentAssistant) return null;
    if (!loaded || loaded.id !== id) return <Dialog open onOpenChange={open => { if (!open) onClose(); }}>
        <DialogContent><DialogHeader><DialogTitle>编辑助手</DialogTitle></DialogHeader>
            <p role={error ? "alert" : "status"}>{error || "正在读取助手形象…"}</p>
        </DialogContent>
    </Dialog>;
    return <ButlerProfileDialog
        key={id}
        name={currentAssistant.assistant.name}
        description={currentAssistant.assistant.description ?? ""}
        appearance={parseAppearance(loaded.config.get("butler_appearance")?.get(appearanceKey(id)))}
        onClose={onClose}
        onSave={async (name, appearance, description) => {
            let detailsSaved = false;
            try {
                const latest = await loadFeatureConfig();
                const updated = { ...currentAssistant, assistant: { ...currentAssistant.assistant, name, description: description ?? "" } };
                await onSave(updated);
                detailsSaved = true;
                onAssistantUpdated(updated);
                await saveFeatureConfig("butler_appearance", {
                    ...Object.fromEntries(latest.get("butler_appearance") ?? []),
                    [appearanceKey(id)]: JSON.stringify(appearance),
                });
                toast.success("名字、描述与形象已保存");
            } catch (error) {
                throw new Error(`${detailsSaved ? "名字与描述已保存，但形象保存失败" : "读取配置或保存助手信息失败"}（助手 ${id}）：${error instanceof Error ? error.message : String(error)}`);
            }
        }} />;
};

export default EditAssistantDialog;
