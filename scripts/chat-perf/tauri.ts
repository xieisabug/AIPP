import hljs from 'highlight.js';
import type { MCPToolCall } from '../../src/data/MCPToolCall';

let calls: MCPToolCall[] = [];
export const diagnostics: string[] = [];
export function setFixtureCalls(value: MCPToolCall[]) { calls = value; diagnostics.length = 0; }
const handlers = new Map<string, Set<(event: { payload: unknown }) => void>>();
export async function listen(name: string, callback: (event: any) => void) {
    const set = handlers.get(name) || new Set();
    set.add(callback); handlers.set(name, set);
    return () => { set.delete(callback); };
}
export async function emit(name: string, payload?: unknown) { handlers.get(name)?.forEach(fn => fn({ payload })); }
export const emitTo = (_target: string, name: string, payload?: unknown) => emit(name, payload);
export async function once(name: string, callback: (event: any) => void) {
    const off = await listen(name, event => { off(); callback(event); }); return off;
}
export const getCurrentWebviewWindow = () => ({ label: 'chat_ui', listen, onFocusChanged: async () => () => {} });
export const getCurrentWindow = getCurrentWebviewWindow;
export const isTauri = () => false;
export const convertFileSrc = (path: string) => { diagnostics.push(`Local asset unavailable: ${path}`); return ''; };
export const writeText = (text: string) => navigator.clipboard.writeText(text);
export const openUrl = async (url: string) => { window.open(url, '_blank', 'noopener,noreferrer'); };
export const save = async () => invoke('benchmark_native_save_unavailable');
export const writeFile = async () => invoke('benchmark_native_write_unavailable');
export const openPath = async () => invoke('benchmark_native_open_unavailable');
export async function invoke<T>(command: string, args: Record<string, any> = {}): Promise<T> {
    switch (command) {
        case 'get_platform': return (navigator.userAgent.includes('Mac') ? 'macos' : 'windows') as T;
        case 'get_all_feature_config': return [
            { feature_code: 'display', key: 'color_mode', value: 'light' },
            { feature_code: 'display', key: 'theme', value: 'default' },
        ] as T;
        case 'get_mcp_tool_call': {
            const call = calls.find(c => c.id === args.callId);
            if (call) return call as T;
            break;
        }
        case 'list_preview_code_requests_for_conversation': return [] as T;
        case 'highlight_code': {
            const code = String(args.code);
            const escaped = code.replaceAll('&','&amp;').replaceAll('<','&lt;').replaceAll('>','&gt;');
            const html = hljs.getLanguage(args.lang) ? hljs.highlight(code, { language: args.lang }).value : escaped;
            return `<pre><code class="hljs">${html}</code></pre>` as T;
        }
        case 'report_preview_code_runtime_error': diagnostics.push(`Preview runtime: ${JSON.stringify(args)}`); return undefined as T;
    }
    const message = `Offline benchmark unsupported command: ${command}`;
    if (!diagnostics.includes(message)) diagnostics.push(message);
    throw new Error(message);
}
