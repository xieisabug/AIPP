import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { fileErrorMessage, mobileFileAction } from "@/services/mobileFileService";
import type { ScannedSkill, SkillWithConfig } from "@/data/Skill";
import { useSkillsMcpValidation } from "@/hooks/useSkillsMcpValidation";
import ConfigPageLoading from "@/components/common/ConfigPageLoading";

export default function MobileSkillsManager({ assistantId }: { assistantId?: number }) {
    const [skills, setSkills] = useState<ScannedSkill[]>([]);
    const [selected, setSelected] = useState<ScannedSkill | null>(null);
    const [content, setContent] = useState("");
    const [compatibility, setCompatibility] = useState("");
    const [bound, setBound] = useState<SkillWithConfig[]>([]);
    const [url, setUrl] = useState("");
    const [busy, setBusy] = useState(false);
    const [query, setQuery] = useState("");
    const [confirmUninstall, setConfirmUninstall] = useState(false);
    // 首次读取完成前不显示「尚未安装技能」
    const [loaded, setLoaded] = useState(false);
    const detailRef = useRef<HTMLElement>(null);
    const { enableOperationMcpAndSkill } = useSkillsMcpValidation();
    const refresh = async () => {
        setSkills(await invoke<ScannedSkill[]>("scan_skills"));
        if (assistantId) setBound(await invoke<SkillWithConfig[]>("get_assistant_skills", { assistantId }));
    };
    useEffect(() => {
        void refresh().catch(error => toast.error(fileErrorMessage(error))).finally(() => setLoaded(true));
    }, [assistantId]);
    useEffect(() => {
        let active = true;
        setContent(""); setCompatibility("");
        if (selected) void Promise.all([
            invoke<{ content: string }>("get_skill_content", { identifier: selected.identifier }),
            invoke<string>("get_mobile_skill_compatibility", { identifier: selected.identifier }),
        ]).then(([result, status]) => { if (active) { setContent(result.content); setCompatibility(status); } })
            .catch(error => { if (active) toast.error(fileErrorMessage(error)); });
        setConfirmUninstall(false);
        // 详情紧跟所选条目展示，并滚动到可见区域
        if (selected) requestAnimationFrame(() => detailRef.current?.scrollIntoView?.({ block: "nearest", behavior: "smooth" }));
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
    const uninstall = (skill: ScannedSkill) => run(async () => {
        await invoke("delete_skill", { identifier: skill.identifier });
        setSelected(null);
        toast.success(`已卸载：${skill.display_name}`);
    });
    const config = selected ? bound.find(item => item.skill?.identifier === selected.identifier) : undefined;
    const isBound = (skill: ScannedSkill) => bound.some(item => item.skill?.identifier === skill.identifier && item.config.is_enabled);
    const keyword = query.trim().toLowerCase();
    const visibleSkills = keyword
        ? skills.filter(skill => skill.display_name.toLowerCase().includes(keyword) || skill.identifier.toLowerCase().includes(keyword))
        : skills;
    if (!loaded) return <ConfigPageLoading />;
    return <div className="flex flex-col gap-4 p-4 safe-bottom overflow-auto h-full">
        <h2 className="text-lg font-semibold">应用内 Skills</h2>
        <p className="text-sm text-muted-foreground">技能保存在此应用内，重启后保留。安装不代表可执行；本地脚本在 Android 不可执行。ZIP 上限 20 MiB，解压上限 50 MiB。</p>
        <div className="flex gap-2"><Button className="min-h-11" disabled={busy} onClick={() => void importZip()}>导入 ZIP</Button><Button className="min-h-11" variant="outline" disabled={busy} onClick={() => void run(refresh)}>刷新</Button></div>
        <Input className="min-h-11" type="url" inputMode="url" enterKeyHint="go" value={url} onChange={event => setUrl(event.target.value)}
            onKeyDown={event => { if (event.key === "Enter" && url.trim() && !busy) { event.preventDefault(); void installUrl(); } }}
            placeholder="HTTPS 技能压缩包地址" aria-label="技能压缩包地址" />
        <Button className="min-h-11" disabled={busy || !url.trim()} onClick={() => void installUrl()}>下载并安装</Button>
        {skills.length > 0 && <Input className="min-h-11" type="search" enterKeyHint="search" value={query} onChange={event => setQuery(event.target.value)} placeholder="搜索技能" aria-label="搜索技能" />}
        {visibleSkills.map(skill => <div key={skill.identifier} className="flex flex-col gap-3 shrink-0">
            <Button className="min-h-11 justify-start" variant={selected?.identifier === skill.identifier ? "secondary" : "outline"} aria-expanded={selected?.identifier === skill.identifier} onClick={() => setSelected(current => current?.identifier === skill.identifier ? null : skill)}>
                <span className="min-w-0 flex-1 truncate text-left">{skill.display_name}</span>
                {assistantId && isBound(skill) && <span className="shrink-0 rounded border border-border px-1.5 text-xs text-muted-foreground">已绑定</span>}
            </Button>
            {selected?.identifier === skill.identifier && <section ref={detailRef} className="space-y-3 rounded-md border border-border p-3">
                <h3 className="text-sm font-medium">{selected.display_name}</h3>
                <p className="text-sm text-muted-foreground" role="status">{compatibility}</p>
                {assistantId && <Button className="min-h-11" disabled={busy} onClick={() => void run(async () => {
                    if (config?.config.is_enabled) {
                        await invoke("toggle_assistant_skill", { configId: config.config.id, isEnabled: false });
                    } else {
                        await enableOperationMcpAndSkill(assistantId, selected.identifier, 0);
                    }
                })}>{config?.config.is_enabled ? "取消绑定" : "启用技能加载工具并绑定"}</Button>}
                {!assistantId && <p className="text-sm text-muted-foreground">在助手设置的 Skills 页面绑定，聊天中可按需加载完整指令。</p>}
                {confirmUninstall
                    ? <div className="flex flex-wrap items-center gap-2">
                        <span className="mr-auto text-sm">确认卸载此技能？</span>
                        <Button className="min-h-11" variant="destructive" disabled={busy} onClick={() => void uninstall(selected)}>确认卸载</Button>
                        <Button className="min-h-11" variant="outline" onClick={() => setConfirmUninstall(false)}>取消</Button>
                    </div>
                    : <Button className="min-h-11" variant="outline" disabled={busy} onClick={() => setConfirmUninstall(true)}>卸载</Button>}
                <pre className="text-sm whitespace-pre-wrap break-words rounded border p-3">{content}</pre>
            </section>}
        </div>)}
        {!skills.length && <p className="text-sm text-muted-foreground">尚未安装技能</p>}
        {skills.length > 0 && !visibleSkills.length && <p className="text-sm text-muted-foreground">没有匹配的技能</p>}
    </div>;
}
