import { useState } from "react";
import { useFeatureConfig } from "@/hooks/feature/useFeatureConfig";
import { Button } from "@/components/ui/button";
import ButlerAvatar from "@/components/butler/ButlerAvatar";
import ButlerProfileDialog from "@/components/butler/ButlerProfileDialog";
import { appearanceKey, defaultAppearance, parseAppearance } from "@/components/butler/butlerAppearance";

export default function AssistantAvatarSettings({ id, name }: { id: number; name: string }) {
    const [editing, setEditing] = useState(false);
    const { featureConfig, loading, loadFeatureConfig, saveFeatureConfig } = useFeatureConfig();
    const appearance = parseAppearance(featureConfig.get("butler_appearance")?.get(appearanceKey(id))) ?? defaultAppearance;
    return <section className="space-y-3" aria-label="助手头像">
        <h3 className="text-sm font-medium">头像</h3>
        <div className="flex items-center gap-4">
            <ButlerAvatar id={id} size={64} appearance={appearance} />
            <Button type="button" variant="outline" disabled={loading} onClick={() => setEditing(true)}>编辑头像</Button>
        </div>
        {editing && <ButlerProfileDialog appearanceOnly name={name} appearance={appearance}
            onClose={() => setEditing(false)} onSave={async (_, next) => {
                try {
                    const latest = await loadFeatureConfig();
                    await saveFeatureConfig("butler_appearance", {
                        ...Object.fromEntries(latest.get("butler_appearance") ?? []),
                        [appearanceKey(id)]: JSON.stringify(next),
                    });
                } catch (error) {
                    throw new Error(`保存助手 ${id} 的头像失败：${String(error)}`);
                }
            }} />}
    </section>;
}
