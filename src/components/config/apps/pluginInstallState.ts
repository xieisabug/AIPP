import { useSyncExternalStore } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

type Progress = { key: string; message: string };
type Task = { pending: boolean; message: string; error?: string };
const tasks = new Map<string, Task>();
const listeners = new Set<() => void>();
const jobs = new Map<string, Promise<void>>();
const subscribe = (listener: () => void) => { listeners.add(listener); return () => { listeners.delete(listener); }; };
function publish(key: string, task: Task) {
    tasks.set(key, task);
    listeners.forEach(listener => listener());
}
export function usePluginInstall(key: string) {
    return useSyncExternalStore(subscribe, () => tasks.get(key));
}
// Tasks belong to the application, not the detail sheet. Reopening must not start another install.
export function installPlugin(preview: {key: string; source: unknown; revision: string; digest: string}): Promise<void> {
    const existing = jobs.get(preview.key);
    if (existing) return existing;
    publish(preview.key, {pending: true, message: '正在准备安装…'});
    const job = (async () => {
        const unlisten = await listen<Progress>('agent-plugin-install-progress', event => {
            if (event.payload.key === preview.key) publish(preview.key, {pending: true, message: event.payload.message});
        });
        try {
            await invoke('install_agent_plugin', {source: preview.source, revision: preview.revision, digest: preview.digest});
            publish(preview.key, {pending: false, message: '安装完成'});
        } finally { unlisten(); }
    })().catch(error => {
        publish(preview.key, {pending: false, message: '', error: String(error)});
        throw error;
    }).finally(() => { jobs.delete(preview.key); });
    jobs.set(preview.key, job);
    return job;
}
