import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { fileErrorMessage, mobileFileAction } from "@/services/mobileFileService";
import type { ScannedSkill, SkillWithConfig } from "@/data/Skill";
import { useSkillsMcpValidation } from "@/hooks/useSkillsMcpValidation";

export default function MobileSkillsManager({ assistantId }: { assistantId?: number }) {
    const [skills, setSkills] = useState<ScannedSkill[]>([]);
    const [selected, setSelected] = useState<ScannedSkill | null>(null);
    const [content, setContent] = useState("");
    const [compatibility, setCompatibility] = useState("");
    const [bound, setBound] = useState<SkillWithConfig[]>([]);
    const [url, setUrl] = useState("");
    const [busy, setBusy] = useState(false);
    const { enableOperationMcpAndSkill } = useSkillsMcpValidation();
    const refresh = async () => {
        setSkills(await invoke<ScannedSkill[]>("scan_skills"));
        if (assistantId) setBound(await invoke<SkillWithConfig[]>("get_assistant_skills", { assistantId }));
    };
    useEffect(() => { void refresh().catch(error => toast.error(fileErrorMessage(error))); }, [assistantId]);
    useEffect(() => {
        let active = true;
        setContent(""); setCompatibility("");
        if (selected) void Promise.all([
            invoke<{ content: string }>("get_skill_content", { identifier: selected.identifier }),
            invoke<string>("get_mobile_skill_compatibility", { identifier: selected.identifier }),
        ]).then(([result, status]) => { if (active) { setContent(result.content); setCompatibility(status); } })
            .catch(error => { if (active) toast.error(fileErrorMessage(error)); });
        return () => { active = false; };
    }, [selected]);
    const run = async (operation: () => Promise<void>) => {
        setBusy(true);
        try { await operation(); await refresh(); } catch (error) { toast.error(fileErrorMessage(error)); }
        finally { setBusy(false); }
    };
    const importZip = () => run(async () => {
        const result = await mobileFileAction("pick", { skill: true });
        if (result.status === "cancelled") return;
        result.errors?.forEach(error => toast.error(error));
        for (const file of result.files ?? []) {
            const names = await invoke<string[]>("install_mobile_skill_archive", { path: file.path });
            toast.success(`已安装：${names.join("、")}；兼容性请查看详情`);
        }
    });
    const installUrl = () => run(async () => {
        const parsed = new URL(url);
        if (parsed.protocol !== "https:") throw new Error("请输入 HTTPS 压缩包地址");
        const source = { type: "zip", url, ref: "main" };
        const plan = await invoke<{ skills: { from: string; to: string; will_replace: boolean }[] }>("inspect_skill_archive_source", { source, dirs: null, useProxy: false });
        if (plan.skills.some(skill => skill.will_replace)) throw new Error("同名技能已存在，本次未覆盖；请先检查旧版本");
        await invoke("install_skill_archive_source", { source, selections: plan.skills.map(({ from, to }) => ({ from, to })), useProxy: false });
        toast.success("安装完成，请检查移动兼容性");
    });
    const config = selected ? bound.find(item => item.skill?.identifier === selected.identifier) : undefined;
    return <div className="flex flex-col gap-4 p-4 safe-bottom overflow-auto h-full">
        <h2 className="text-lg font-semibold">应用内 Skills</h2>
        <p className="text-sm text-muted-foreground">技能保存在此应用内，重启后保留。安装不代表可执行；本地脚本在 Android 不可执行。ZIP 上限 20 MiB，解压上限 50 MiB。</p>
        <div className="flex gap-2"><Button className="min-h-11" disabled={busy} onClick={() => void importZip()}>导入 ZIP</Button><Button className="min-h-11" variant="outline" disabled={busy} onClick={() => void run(refresh)}>刷新</Button></div>
        <Input className="min-h-11" value={url} onChange={event => setUrl(event.target.value)} placeholder="HTTPS 技能压缩包地址" aria-label="技能压缩包地址" />
        <Button className="min-h-11" disabled={busy || !url.trim()} onClick={() => void installUrl()}>下载并安装</Button>
        {skills.map(skill => <Button key={skill.identifier} className="min-h-11 justify-start shrink-0" variant={selected?.identifier === skill.identifier ? "secondary" : "outline"} onClick={() => setSelected(skill)}>{skill.display_name}</Button>)}
        {!skills.length && <p>尚未安装技能</p>}
        {selected && <section className="space-y-3">
            <h3 className="font-semibold">{selected.display_name}</h3>
            <p className="text-sm text-muted-foreground" role="status">{compatibility}</p>
            {assistantId && <Button className="min-h-11" disabled={busy} onClick={() => void run(async () => {
                if (config?.config.is_enabled) {
                    await invoke("toggle_assistant_skill", { configId: config.config.id, isEnabled: false });
                } else {
                    await enableOperationMcpAndSkill(assistantId, selected.identifier, 0);
                }
            })}>{config?.config.is_enabled ? "取消绑定" : "启用技能加载工具并绑定"}</Button>}
            {!assistantId && <p className="text-sm text-muted-foreground">在助手设置的 Skills 页面绑定，聊天中可按需加载完整指令。</p>}
            <pre className="text-sm whitespace-pre-wrap break-words rounded border p-3">{content}</pre>
        </section>}
    </div>;
}
