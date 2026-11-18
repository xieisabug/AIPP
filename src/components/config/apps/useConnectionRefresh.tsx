import { useCallback, useEffect, useRef, useState } from 'react';
import { Button } from '@/components/ui/button';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { reviewRefresh, type RefreshPreview } from './connectionManagement';

function changeLabel(field: string) {
    const labels: Record<string, string> = {
        properties: '参数', required: '必填项', description: '说明', type: '类型',
        enum: '可选值', default: '默认值', items: '数组元素', additionalProperties: '额外参数',
        readOnlyHint: '只读声明', destructiveHint: '删除或覆盖数据声明',
        idempotentHint: '重复调用声明', openWorldHint: '访问外部服务声明',
    };
    return field.split('.').map(part => labels[part] ?? part).join(' · ');
}

export function useConnectionRefresh() {
    const [preview, setPreview] = useState<RefreshPreview>();
    const mounted = useRef(true);
    const pending = useRef<((accepted: boolean) => void) | undefined>(undefined);
    const finish = useCallback((accepted: boolean) => {
        pending.current?.(accepted);
        pending.current = undefined;
        setPreview(undefined);
    }, []);
    useEffect(() => {
        mounted.current = true;
        return () => { mounted.current = false; pending.current?.(false); pending.current = undefined; };
    }, []);
    const confirm = useCallback((diff: RefreshPreview) => new Promise<boolean>(resolve => {
        if (!mounted.current) { resolve(false); return; }
        pending.current?.(false);
        pending.current = resolve;
        setPreview(diff);
    }), []);
    const refreshConnection = useCallback((serverId: number) => reviewRefresh(serverId, confirm), [confirm]);
    const groups = preview ? [
        { title: '更新', names: preview.changed, hint: '保留现有启用状态、自动运行设置和助手授权。' },
        { title: '新增', names: preview.added, hint: '默认关闭，可在工具页开启。' },
        { title: '移除', names: preview.removed, hint: '将删除这些工具及其授权。' },
    ] : [];
    const knownNames = new Set(groups.flatMap(group => group.names));
    const extraPermissions = preview?.permissions?.filter(line => !knownNames.has(line.split('：')[0])) ?? [];
    const refreshDialog = <Dialog open={!!preview} onOpenChange={open => { if (!open) finish(false); }}>
        <DialogContent className="flex max-h-[85dvh] flex-col overflow-hidden sm:max-w-xl">
            <DialogHeader className="shrink-0 pr-6">
                <DialogTitle>确认工具更新</DialogTitle>
                <DialogDescription>服务提供了新的工具信息。请查看变化后应用更新。</DialogDescription>
            </DialogHeader>
            <div className="min-h-0 flex-1 space-y-5 overflow-y-auto overscroll-contain pr-1">
                {groups.filter(group => group.names.length > 0).map(group => <section key={group.title} className="space-y-2">
                    <h3 className="text-sm font-medium">{group.title} <span className="text-muted-foreground">{group.names.length} 个工具</span></h3>
                    <p className="text-xs leading-5 text-muted-foreground">{group.hint}</p>
                    <ul className="divide-y rounded-lg border">
                        {group.names.map(name => {
                            const details = preview?.permissions?.filter(line => line.startsWith(name + '：')) ?? [];
                            return <li key={name} className="space-y-1 px-3 py-3">
                                <p className="break-all font-mono text-sm">{name}</p>
                                {preview?.details?.[name]?.map((change, index) => <details key={index} className="rounded border bg-muted/30">
                                    <summary className="cursor-pointer px-3 py-2 text-sm">
                                        {changeLabel(change.field)}
                                        <span className="ml-2 text-xs text-muted-foreground">{change.before === null ? '新增' : change.after === null ? '移除' : '已修改'}</span>
                                    </summary>
                                    <div className="grid gap-3 border-t p-3 sm:grid-cols-2">
                                        {[['更新前', change.before], ['更新后', change.after]].map(([label, value]) => <div key={label}>
                                            <p className="mb-1 text-xs font-medium text-muted-foreground">{label}</p>
                                            <pre className="max-h-60 overflow-y-auto whitespace-pre-wrap break-words text-xs leading-5">{value === null ? '未提供' : value === '' ? '（空）' : value}</pre>
                                        </div>)}
                                    </div>
                                </details>)}
                                {details.map((line, index) => <p key={index} className="break-words text-xs leading-5 text-muted-foreground">
                                    {line.slice(name.length + 1)}
                                </p>)}
                            </li>;
                        })}
                    </ul>
                </section>)}
                {extraPermissions.length > 0 && <section className="space-y-2">
                    <h3 className="text-sm font-medium">权限说明</h3>
                    {extraPermissions.map((line, index) => <p key={index} className="break-words text-sm text-muted-foreground">{line}</p>)}
                </section>}
            </div>
            <DialogFooter className="shrink-0 border-t pt-4">
                <Button variant="outline" className="min-h-11" onClick={() => finish(false)}>取消</Button>
                <Button className="min-h-11" onClick={() => finish(true)}>应用更新</Button>
            </DialogFooter>
        </DialogContent>
    </Dialog>;
    return { refreshConnection, refreshDialog };
}
