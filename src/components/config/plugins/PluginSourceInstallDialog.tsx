import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { Loader2, PackagePlus } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { Input } from '@/components/ui/input';
import { Dialog, CompactDialogBody, CompactDialogContent, CompactDialogHeader } from '@/components/ui/dialog';
import { PluginTypeIcon } from './PluginCard';
import { parsePluginSourceInput, pluginTypeLabel, primaryPluginType, type PluginInstallRecipeDir, type PluginInstallRecipeSource } from './pluginCatalog';

interface PlanPlugin {
    from: string;
    to: string;
    code: string;
    name: string;
    version: string;
    pluginType: string[];
    permissions: string[];
    willReplace: boolean;
    installedVersion?: string | null;
    validation: { isInstallable: boolean; warnings: string[]; errors: string[] };
}
interface Inspection { source: PluginInstallRecipeSource; sourceLabel: string; archiveSha256: string; plugins: PlanPlugin[] }

/** 「安装 → 从来源安装」：先读取来源、列出里面的插件，再按勾选安装。 */
export default function PluginSourceInstallDialog({ open: isOpen, onOpenChange, onInstalled, mobile }: {
    open: boolean; onOpenChange: (open: boolean) => void; onInstalled: () => Promise<void>; mobile: boolean;
}) {
    const [input, setInput] = useState('');
    const [useProxy, setUseProxy] = useState(false);
    const [busy, setBusy] = useState<'' | 'reading' | 'installing'>('');
    const [error, setError] = useState('');
    const [inspection, setInspection] = useState<Inspection>();
    const [selected, setSelected] = useState<Set<string>>(new Set());

    const reset = () => { setInspection(undefined); setSelected(new Set()); setError(''); };
    const close = (next: boolean) => { if (busy) return; if (!next) { reset(); setInput(''); } onOpenChange(next); };

    const read = async (value = input) => {
        let parsed: { source: PluginInstallRecipeSource; dirs?: PluginInstallRecipeDir[] };
        try { parsed = parsePluginSourceInput(value); } catch (e) { setError(e instanceof Error ? e.message : String(e)); return; }
        reset(); setBusy('reading');
        try {
            const result = await invoke<Inspection>('inspect_plugin_archive_source', { source: parsed.source, dirs: parsed.dirs ?? null, expectedSha256: null, useProxy });
            setInspection(result);
            setSelected(new Set(result.plugins.filter(p => p.validation.isInstallable).map(p => p.to)));
            if (!result.plugins.length) setError('这个来源里没有找到插件（需要包含 plugin.json）。');
        } catch (e) {
            setError(`读取来源失败：${String(e)}${useProxy ? '' : '。可以勾选「使用代理」后重试。'}`);
        } finally { setBusy(''); }
    };

    const pickZip = async () => {
        const file = await open({ multiple: false, directory: false, filters: [{ name: 'ZIP 插件包', extensions: ['zip'] }] }).catch(e => { toast.error(`选择 ZIP 失败：${String(e)}`); return null; });
        if (typeof file === 'string') { setInput(file); void read(file); }
    };

    const install = async () => {
        if (!inspection) return;
        const selections = inspection.plugins.filter(p => selected.has(p.to)).map(p => ({ from: p.from, to: p.to }));
        setBusy('installing'); setError('');
        try {
            // 带上读取时的 sha256：装进去的必须就是刚才看过的那个包。
            const result = await invoke<{ installedPlugins: PlanPlugin[] }>('install_plugin_archive_source', {
                source: inspection.source, selections, expectedSha256: inspection.archiveSha256, useProxy, enableAfterInstall: true, officialId: null,
            });
            toast.success(`已安装 ${result.installedPlugins.length} 个插件`);
            await onInstalled();
            setBusy(''); close(false);
        } catch (e) {
            setError(`安装失败：${String(e)}`);
            setBusy('');
        }
    };

    const count = selected.size;
    return <Dialog open={isOpen} onOpenChange={close}>
        <CompactDialogContent className={`sm:max-w-xl ${mobile ? '[&_button]:min-h-11' : ''}`}>
            <CompactDialogHeader icon={<PackagePlus className="h-4 w-4" />} title="从来源安装" subtitle="GitHub 仓库、ZIP 链接或本地 ZIP"
                description="非官方插件不会经过 AIPP 审核，请只安装可信来源。" />
            <CompactDialogBody className="space-y-4">
                <div className="flex gap-2">
                    <Input aria-label="插件来源" placeholder="owner/repo#main 或 https://…/plugin.zip" value={input} disabled={!!busy}
                        onChange={e => { setInput(e.target.value); setError(''); }} onKeyDown={e => { if (e.key === 'Enter') void read(); }} />
                    <Button variant="outline" disabled={!!busy || !input.trim()} onClick={() => void read()}>{busy === 'reading' && <Loader2 className="h-4 w-4 animate-spin" />}读取</Button>
                </div>
                <div className="flex flex-wrap items-center justify-between gap-2 text-xs text-muted-foreground">
                    <span>支持 owner/repo、owner/repo#ref、GitHub 仓库或 tree 链接、ZIP 链接。</span>
                    <span className="flex items-center gap-3">
                        <label className="flex items-center gap-1.5"><Checkbox checked={useProxy} onCheckedChange={v => setUseProxy(v === true)} disabled={!!busy} />使用代理</label>
                        {!mobile && <Button variant="link" size="sm" className="h-auto p-0 text-xs" disabled={!!busy} onClick={() => void pickZip()}>选择本地 ZIP</Button>}
                    </span>
                </div>
                {inspection && inspection.plugins.length > 0 && <section className="space-y-2">
                    <p className="text-xs text-muted-foreground">{inspection.sourceLabel} 包含 {inspection.plugins.length} 个插件</p>
                    <div className="divide-y rounded-lg border">{inspection.plugins.map(plan => {
                        const installable = plan.validation.isInstallable;
                        const state = !installable ? '不可安装' : plan.willReplace ? `替换 ${plan.installedVersion ?? '已安装版本'}` : '新安装';
                        return <label key={plan.to} className={`flex items-center gap-3 px-3 py-2.5 ${installable ? 'cursor-pointer' : 'opacity-60'}`}>
                            <Checkbox aria-label={`安装 ${plan.name}`} disabled={!installable || !!busy} checked={selected.has(plan.to)}
                                onCheckedChange={v => setSelected(prev => { const next = new Set(prev); if (v === true) next.add(plan.to); else next.delete(plan.to); return next; })} />
                            <PluginTypeIcon type={primaryPluginType(plan.pluginType)} />
                            <span className="min-w-0 flex-1">
                                <span className="block truncate text-sm font-medium">{plan.name} <span className="font-normal text-muted-foreground">{plan.version}</span></span>
                                <span className={`block truncate text-xs ${installable ? 'text-muted-foreground' : 'text-destructive'}`}>
                                    {installable ? [pluginTypeLabel(primaryPluginType(plan.pluginType)), ...plan.validation.warnings].join(' · ') : plan.validation.errors.join('；')}
                                </span>
                            </span>
                            <span className={`shrink-0 text-xs ${plan.willReplace && installable ? 'text-amber-600 dark:text-amber-400' : 'text-muted-foreground'}`}>{state}</span>
                        </label>;
                    })}</div>
                </section>}
                {error && <p role="alert" className="break-words text-sm text-destructive">{error}</p>}
                <div className="flex justify-end gap-2 pt-1">
                    <Button variant="outline" disabled={!!busy} onClick={() => close(false)}>取消</Button>
                    <Button disabled={!inspection || !count || !!busy} onClick={() => void install()}>{busy === 'installing' && <Loader2 className="h-4 w-4 animate-spin" />}{count ? `安装 ${count} 个插件` : '安装'}</Button>
                </div>
            </CompactDialogBody>
        </CompactDialogContent>
    </Dialog>;
}
