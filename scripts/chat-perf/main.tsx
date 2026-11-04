import { useEffect, useMemo, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { Download, Play, RotateCcw } from 'lucide-react';
import VirtuosoMessageList from '../../src/components/conversation/VirtuosoMessageList';
import { AntiLeakageProvider } from '../../src/contexts/AntiLeakageContext';
import CodeThemeLoader from '../../src/components/CodeThemeLoader';
import { useMessageGroups } from '../../src/hooks/useMessageGroups';
import { useMessageProcessing } from '../../src/hooks/useMessageProcessing';
import { runScrollPerformanceProbe, waitForCondition } from '../../src/utils/chatScrollPerf';
import type { Message } from '../../src/data/Conversation';
import type { MCPToolCall } from '../../src/data/MCPToolCall';
import { diagnostics, setFixtureCalls } from './tauri';
import '../../src/App.css';
import 'katex/dist/katex.min.css';
import './style.css';

interface Case { id: number; name: string; file: string; sha256: string; messages: number; chars: number; tools: number; previews: number; codeFences: number; selectedFor: string[] }
interface Fixture { schemaVersion: number; conversation: { id: number; name: string }; messages: Message[]; toolCalls: MCPToolCall[] }
const noop = () => {};
const emptyMap = new Map();
const emptySet = new Set<number>();
const heights = new Map<string, { min: number; max: number; changes: number }>();
const errors: string[] = [];
window.addEventListener('error', e => errors.push(e.message));
window.addEventListener('unhandledrejection', e => errors.push(String(e.reason)));
window.__AIPP_CHAT_PERF_CAPTURE__ = {
    recordVirtualRowHeight(key: string, height: number) {
        const old = heights.get(key);
        heights.set(key, { min: Math.min(old?.min ?? height, height), max: Math.max(old?.max ?? height, height), changes: (old?.changes ?? 0) + 1 });
    },
};

function Chat({ fixture, expanded }: { fixture: Fixture; expanded: boolean }) {
    const container = useRef<HTMLDivElement>(null);
    const [, setShining] = useState(new Set<number>());
    const groups = useMessageGroups({ allDisplayMessages: fixture.messages, groupMergeMap: emptyMap });
    const { allDisplayMessages: messages } = useMessageProcessing({ messages: fixture.messages, streamingMessages: emptyMap,
        generationGroups: groups.generationGroups, groupRootMessageIds: groups.groupRootMessageIds,
        getMessageVersionInfo: groups.getMessageVersionInfo });
    const toolStates = useMemo(() => new Map(fixture.toolCalls.map(call => [call.id, { ...call, call_id: call.id }])), [fixture]);
    const [reasoning, setReasoning] = useState(() => new Map(fixture.messages.map(m => [m.id, expanded])));
    return <div ref={container} className="benchmark-scroll" data-aipp-slot="chat-conversation-scroll">
        <VirtuosoMessageList conversationId={String(fixture.conversation.id)}
            allDisplayMessages={messages} streamingMessages={emptyMap} shiningMessageIds={emptySet}
            shiningMcpCallId={null} reasoningExpandStates={reasoning} mcpToolCallStates={toolStates}
            generationGroups={groups.generationGroups} selectedVersions={groups.selectedVersions}
            getGenerationGroupControl={groups.getGenerationGroupControl} handleGenerationVersionChange={groups.handleGenerationVersionChange}
            onCodeRun={noop} onMessageRegenerate={noop} onMessageEdit={noop} onMessageFork={noop}
            onToggleReasoningExpand={id => setReasoning(prev => new Map(prev).set(id, !prev.get(id)))}
            scrollContainerRef={container} pendingScrollMessageId={null} clearPendingScrollMessageId={noop}
            setShiningMessageIds={setShining} smartScroll={noop} />
    </div>;
}

function App() {
    const [cases, setCases] = useState<Case[]>([]);
    const [selected, setSelected] = useState('');
    const [fixture, setFixture] = useState<Fixture | null>(null);
    const [error, setError] = useState('');
    const [busy, setBusy] = useState(false);
    const [expanded, setExpanded] = useState(false);
    const [seconds, setSeconds] = useState(15);
    const [epoch, setEpoch] = useState(0);
    const [results, setResults] = useState<any[]>([]);
    const selectedCase = cases.find(c => String(c.id) === selected);
    useEffect(() => {
        fetch('/manifest.json').then(async r => {
            if (!r.ok) throw new Error('No fixture manifest. Run the read-only exporter first.');
            return r.json();
        }).then(manifest => { setCases(manifest.cases); setSelected(String(manifest.cases[0]?.id ?? '')); }).catch(e => setError(String(e)));
    }, []);
    useEffect(() => {
        if (!selectedCase) return;
        let cancelled = false;
        setFixture(null); setError('');
        fetch(`/${selectedCase.file}`).then(async r => {
            if (!r.ok) throw new Error(`Fixture HTTP ${r.status}`);
            const bytes = await r.arrayBuffer();
            const hash = Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256', bytes)), b => b.toString(16).padStart(2,'0')).join('');
            if (hash !== selectedCase.sha256) throw new Error('Fixture SHA-256 mismatch');
            return JSON.parse(new TextDecoder().decode(bytes)) as Fixture;
        }).then(data => {
            if (cancelled) return;
            if (data.schemaVersion !== 1 || !data.messages.length) throw new Error('Unsupported or empty fixture');
            const attachmentTypes = ['', 'Image', 'Text', 'PDF', 'Word', 'PowerPoint', 'Excel', 'Skill'];
            const messages = data.messages.map(message => ({ ...message,
                attachment_list: message.attachment_list?.map(attachment => ({ ...attachment,
                    attachment_type: typeof attachment.attachment_type === 'number'
                        ? attachmentTypes[attachment.attachment_type] : attachment.attachment_type,
                })),
            }));
            setFixtureCalls(data.toolCalls); errors.length = 0; heights.clear(); setFixture({ ...data, messages });
        }).catch(e => { if (!cancelled) setError(String(e)); });
        return () => { cancelled = true; };
    }, [selectedCase]);
    async function run() {
        if (!fixture || !selectedCase || busy) return;
        setBusy(true); setError(''); heights.clear();
        let hidden = document.hidden;
        let coverageTimer: number | undefined;
        const previewHosts = new Set<Element>();
        let maxMountedPreviews = 0;
        const visibility = () => { if (document.hidden) hidden = true; };
        document.addEventListener('visibilitychange', visibility);
        try {
            await document.fonts.ready;
            await waitForCondition(() => !!document.querySelector('[data-message-item]'), { timeoutMs: 30000 });
            const container = document.querySelector<HTMLElement>('[data-aipp-slot="chat-conversation-scroll"]')!;
            await new Promise(resolve => setTimeout(resolve, 1000));
            coverageTimer = window.setInterval(() => {
                const mounted = [...container.querySelectorAll('[data-testid="preview-code-host"]')]
                    .filter(host => !!host.shadowRoot?.querySelector('.aipp-preview-code-root')?.childElementCount);
                maxMountedPreviews = Math.max(maxMountedPreviews, mounted.length);
                mounted.forEach(host => previewHosts.add(host));
            }, 250);
            const metrics = await runScrollPerformanceProbe(container, { durationMs: seconds * 1000, includeReturnTrip: true });
            const result = { schemaVersion: 1, timestamp: new Date().toISOString(),
                fixtureId: selectedCase.id, fixtureSha256: selectedCase.sha256, fixtureStats: selectedCase,
                environment: { userAgent: navigator.userAgent, viewport: [innerWidth, innerHeight], devicePixelRatio,
                    hardwareConcurrency: navigator.hardwareConcurrency, reducedMotion: matchMedia('(prefers-reduced-motion: reduce)').matches },
                mode: 'offline-virtuoso-highlightjs', expandedReasoning: expanded, seconds, runIndex: results.length + 1,
                valid: !hidden && !errors.length && !diagnostics.length && metrics.sampleCount > 0 && metrics.maxScrollTop > 0,
                hiddenDuringRun: hidden, errors: [...errors], diagnostics: [...diagnostics], metrics,
                previewCoverage: { observedMountedHostInstances: previewHosts.size, maxMountedPreviews,
                    note: 'Host instances may include remounts; this is not a unique tool-call count.' },
                rowHeightDrift: [...heights].map(([key,v]) => ({ key, ...v, drift: v.max-v.min })).sort((a,b) => b.drift-a.drift),
                limitations: ['No Rust IPC, database, live generation or plugin loading.', 'Highlight.js replaces Rust syntect equally in all browser runs.',
                    'Raw persisted messages; historical parent_id regeneration and native activity reconstruction are not replayed.',
                    'External preview resource preparation and local attachment files are not available. Unsupported commands invalidate the run.'] };
            setResults(prev => [...prev, result]);
            (window as any).__CHAT_BROWSER_PERF_RESULT__ = result;
        } catch (e) { setError(String(e)); }
        finally { clearInterval(coverageTimer); document.removeEventListener('visibilitychange', visibility); setBusy(false); }
    }
    function download() {
        const url = URL.createObjectURL(new Blob([JSON.stringify(results, null, 2)], { type: 'application/json' }));
        const a = document.createElement('a'); a.href = url; a.download = `chat-perf-${Date.now()}.json`; a.click(); URL.revokeObjectURL(url);
    }
    return <AntiLeakageProvider enabled={false}><CodeThemeLoader>
        <main className="benchmark" data-aipp-window="chat_ui">
            <header><strong>AIPP Chat Performance</strong>
                <select aria-label="Conversation" disabled={busy} value={selected} onChange={e => setSelected(e.target.value)}>
                    {cases.map(c => <option key={c.id} value={c.id}>#{c.id} {c.name}</option>)}
                </select>
                <label><input type="checkbox" checked={expanded} disabled={busy} onChange={e => { setExpanded(e.target.checked); setEpoch(n=>n+1); }} /> Reasoning</label>
                <label>Seconds <input type="number" min="5" max="120" value={seconds} disabled={busy} onChange={e => setSeconds(Math.max(5, Math.min(120, Number(e.target.value) || 15)))} /></label>
                <button title="Run scroll benchmark" disabled={!fixture || busy} onClick={run}><Play size={16} />{busy ? 'Running' : 'Run'}</button>
                <button title="Remount conversation" disabled={busy} onClick={() => setEpoch(n=>n+1)}><RotateCcw size={16}/></button>
                <button title="Download results" disabled={!results.length || busy} onClick={download}><Download size={16}/></button>
            </header>
            {selectedCase && <div className="stats">{selectedCase.messages} messages / {selectedCase.tools} tools / {selectedCase.previews} previews / {selectedCase.codeFences} fences / {selectedCase.chars.toLocaleString()} characters / {selectedCase.selectedFor.join(', ')}</div>}
            {error && <div role="alert" className="error">{error}</div>}
            {fixture && <Chat key={`${selected}-${epoch}`} fixture={fixture} expanded={expanded} />}
            <footer>{results.length ? <table><thead><tr><th>Run</th><th>Case</th><th>P95 ms</th><th>Worst ms</th><th>Blank %</th><th>Status</th></tr></thead>
                <tbody>{results.slice(-5).map((r,i) => <tr key={i}><td>{r.runIndex}</td><td>{r.fixtureId}</td><td>{r.metrics.p95FrameMs}</td><td>{r.metrics.worstFrameMs}</td><td>{(r.metrics.maxBlankViewportRatio*100).toFixed(1)}</td><td>{r.valid ? 'Valid' : 'Incomplete'}</td></tr>)}</tbody></table> : 'No results'}</footer>
        </main>
    </CodeThemeLoader></AntiLeakageProvider>;
}
createRoot(document.getElementById('root')!).render(<App />);
