import { useCallback, useEffect, useState, type ReactNode } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Plus, RefreshCcw } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Switch } from '@/components/ui/switch';
import { InfoList } from '../catalog/DetailParts';
import type { InstalledPlugin } from './pluginCatalog';

interface PluginDetailItem {
    pluginDir: string;
    entryPath: string;
    entrySha256?: string | null;
    runtime: { type: string; entry: string; protocol?: string | null; checksum?: string | null };
}
interface PluginConfigItem { configId: number; configKey: string; configValue?: string | null }
interface HookRegistration { id: number; hookName: string; hookKind: string; priority: number; timeoutMs: number; failurePolicy: string; isActive: boolean }
interface HookAuditLog { id: number; pluginId: number; hookName: string; conversationId?: number | null; status: string; action?: string | null; durationMs?: number | null; error?: string | null; createdAt: string }

// 系统内部写入的配置键，不在 KV 里展示，避免误改。
const INTERNAL_CONFIG_KEYS = new Set(['plugin_type']);
const mono = 'font-mono text-xs';

/** 「高级」：运行时、配置项、Hook 调试。只给已安装插件。 */
export default function PluginAdvancedPanel({ plugin }: { plugin: InstalledPlugin }) {
    const [detail, setDetail] = useState<PluginDetailItem | null>();
    const [configs, setConfigs] = useState<PluginConfigItem[]>([]);
    const [hooks, setHooks] = useState<HookRegistration[]>([]);
    const [logs, setLogs] = useState<HookAuditLog[]>([]);
    const [loading, setLoading] = useState(false);
    const [key, setKey] = useState('');
    const [value, setValue] = useState('');
    const [saving, setSaving] = useState(false);

    const load = useCallback(async () => {
        setLoading(true);
        try {
            const [nextDetail, nextConfigs, nextHooks, nextLogs] = await Promise.all([
                plugin.isInstalled ? invoke<PluginDetailItem>('get_plugin_detail', { code: plugin.code }).catch(() => null) : Promise.resolve(null),
                invoke<PluginConfigItem[]>('get_plugin_config', { pluginId: plugin.pluginId }),
                invoke<HookRegistration[]>('get_plugin_hook_registrations', { pluginId: plugin.pluginId }),
                invoke<HookAuditLog[]>('list_plugin_hook_audit_logs', { limit: 100 }),
            ]);
            setDetail(nextDetail);
            setConfigs(nextConfigs.filter(item => !INTERNAL_CONFIG_KEYS.has(item.configKey)));
            setHooks(nextHooks);
            setLogs(nextLogs.filter(log => log.pluginId === plugin.pluginId));
        } catch (error) {
            toast.error(`加载插件调试信息失败：${String(error)}`);
        } finally {
            setLoading(false);
        }
    }, [plugin.code, plugin.isInstalled, plugin.pluginId]);
    useEffect(() => { void load(); }, [load]);

    const save = async () => {
        if (!key.trim()) { toast.error('请填写配置键'); return; }
        setSaving(true);
        try {
            await invoke('set_plugin_config', { pluginId: plugin.pluginId, key: key.trim(), value: value.trim() ? value : null });
            setKey(''); setValue('');
            toast.success('配置项已保存');
            await load();
        } catch (error) {
            toast.error(`保存配置项失败：${String(error)}`);
        } finally {
            setSaving(false);
        }
    };

    const runtime = detail?.runtime ?? plugin.runtime;
    return <div className="space-y-8">
        <InfoList title="运行时" rows={[
            ['运行时', runtime ? `${runtime.type.toUpperCase()}${runtime.protocol ? ` · ${runtime.protocol}` : ''}` : '—'],
            ['入口', <span className={mono}>{runtime?.entry ?? '—'}</span>],
            ...(detail?.entrySha256 ? [['入口校验', <span className={`${mono} break-all`}>{detail.entrySha256}</span>] as [string, ReactNode]] : []),
            ['目录', <span className={`${mono} break-all`}>{detail?.pluginDir ?? plugin.code}</span>],
        ]} />

        <section className="space-y-3">
            <h4 className="text-sm font-medium">配置项 <span className="font-normal text-muted-foreground">{configs.length}</span></h4>
            <div className="divide-y rounded-lg border text-sm">
                {configs.map(item => <div key={item.configId} className="flex items-start justify-between gap-4 px-4 py-2.5">
                    <span className={mono}>{item.configKey}</span>
                    <span className={`${mono} min-w-0 break-all text-right text-muted-foreground`}>{item.configValue ?? '(空)'}</span>
                </div>)}
                {!configs.length && <p className="px-4 py-3 text-muted-foreground">{loading ? '正在加载…' : '暂无配置项。'}</p>}
                <div className="flex flex-wrap gap-2 px-4 py-3">
                    <Input className="h-8 min-w-32 flex-1" aria-label="配置键" placeholder="键" value={key} onChange={e => setKey(e.target.value)} />
                    <Input className="h-8 min-w-40 flex-[2]" aria-label="配置值" placeholder="值（留空即清除）" value={value} onChange={e => setValue(e.target.value)} />
                    <Button size="sm" variant="outline" disabled={saving} onClick={() => void save()}><Plus className="h-4 w-4" />保存</Button>
                </div>
            </div>
        </section>

        <section className="space-y-3">
            <div className="flex items-center justify-between gap-3">
                <h4 className="text-sm font-medium">Hook 调试</h4>
                <Button size="sm" variant="ghost" disabled={loading} onClick={() => void load()}><RefreshCcw className={loading ? 'h-4 w-4 animate-spin' : 'h-4 w-4'} />刷新</Button>
            </div>
            <div className="divide-y rounded-lg border text-sm">
                {hooks.map(hook => <div key={hook.id} className="flex items-center justify-between gap-4 px-4 py-2.5">
                    <span className={mono}>{hook.hookName}</span>
                    <span className="flex items-center gap-3 text-xs text-muted-foreground">
                        {hook.hookKind} · 优先级 {hook.priority} · 超时 {hook.timeoutMs}ms · {hook.failurePolicy === 'log' ? '失败时记录后继续' : `失败时 ${hook.failurePolicy}`}
                        <Switch checked={hook.isActive} disabled aria-label={`${hook.hookName} ${hook.isActive ? '已启用' : '已停用'}`} />
                    </span>
                </div>)}
                {!hooks.length && <p className="px-4 py-3 text-muted-foreground">这个插件没有注册 Hook。</p>}
            </div>
            {logs.length > 0 && <div className="divide-y rounded-lg border text-xs">
                {logs.map(log => <div key={log.id} className="grid grid-cols-[9rem_4rem_1fr] gap-3 px-4 py-2 text-muted-foreground">
                    <span>{log.createdAt}</span>
                    <span className={log.status === 'success' ? 'text-emerald-600 dark:text-emerald-400' : 'text-destructive'}>{log.status === 'success' ? '成功' : log.status}</span>
                    <span className="min-w-0 break-words">{log.hookName}{log.conversationId ? ` · 会话 #${log.conversationId}` : ''}{typeof log.durationMs === 'number' ? ` · ${log.durationMs}ms` : ''}{log.error ? ` · ${log.error}` : ''}</span>
                </div>)}
            </div>}
        </section>
    </div>;
}
