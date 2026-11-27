import { invoke } from "@tauri-apps/api/core";
import { confirmDialog } from "@/components/common/ConfirmHost";
import { SelectGroup, SelectItem, SelectLabel } from "@/components/ui/select";
import type { ModelForSelect } from "@/hooks/useFilteredModels";

export function ButlerModelOptions({ api, agents }: { api: ModelForSelect[]; agents: ModelForSelect[] }) {
    const groups = [{ label: "API 模型", models: api }, { label: "本机智能体", models: agents }];
    return <>{groups.map(({ label, models }) => {
        return models.length > 0 && <SelectGroup key={label}>
            <SelectLabel>{label}</SelectLabel>
            {models.map(model => <SelectItem key={`${model.code}%%${model.llm_provider_id}`} value={`${model.code}%%${model.llm_provider_id}`}>
                {model.name}
            </SelectItem>)}
        </SelectGroup>;
    })}</>;
}

export async function saveButlerModel(next: string, save: () => Promise<unknown>): Promise<boolean> {
    const changed = await invoke<boolean>("butler_channel_change_required", { next });
    if (changed && !await confirmDialog({
        title: "切换总管家模型", description: "切换总管家的大脑会开始一段新的主会话，之前的对话可以在历史里查看。",
        confirmText: "切换并开始新会话",
    })) return false;
    await save();
    return true;
}
