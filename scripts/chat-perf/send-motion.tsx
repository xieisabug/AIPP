// Synthetic send harness: real list, bubbles, virtualization and viewport sizing;
// no backend/model calls. Open the production-built send-motion.html.
import { analyzeTrajectory } from './send-trajectory';
import { Profiler, useMemo, useRef, useState } from 'react';
import { flushSync } from 'react-dom';
import { createRoot } from 'react-dom/client';
import InputArea from '../../src/components/conversation/InputArea';
import VirtuosoMessageList from '../../src/components/conversation/VirtuosoMessageList';
import { useScrollManagement } from '../../src/hooks/useScrollManagement';
import { AntiLeakageProvider } from '../../src/contexts/AntiLeakageContext';
import type { Message } from '../../src/data/Conversation';
import '../../src/App.css';
import './style.css';
const noop = () => {};
const map = new Map();
const set = new Set<number>();
let serial = 1000;
function message(id: number, type: string, content: string): Message {
    return { id, conversation_id: 1, message_type: type, content, attachment_list: [],
        created_time: new Date('2026-11-18T00:00:00Z'), start_time: null, regenerate: null, finish_time: null, llm_model_id: null,
        token_count: 0, input_token_count: 0, output_token_count: 0,
        generation_group_id: null };
}
const delay = (ms: number) => new Promise<void>(resolve => setTimeout(resolve, ms));
const history = (count: number) => Array.from({ length: count }, (_, i) => message(i + 1,
    i % 2 ? 'response' : 'user', `历史消息 ${i + 1}。\n\n${'用于检查长历史的虚拟列表定位。'.repeat(i % 5 + 1)}`));
interface Frame { phase: string; clientHeight: number; anchorTop: number | null; bubbleTop: number | null; thumbPosition: number; thumbSize: number; nodes: number; ms: number; top: number | null; scroll: number; height: number; opacity: number | null }
interface Result { trajectory: ReturnType<typeof analyzeTrajectory>; scenario: string; actionMs: number; finalTop: number | null; scrollDrift: number | null; animationCalls: number; frames: number; maxFrameMs: number; p95FrameMs: number; passed: boolean; samples: Frame[]; historyCommitMs: number | null; longTasks: {ms:number; duration:number}[]; imageLoaded: boolean; commits: {phase:string; actualDuration:number; startTime:number; commitTime:number}[] }
function Chat() {
    const realComposer = new URLSearchParams(location.search).has('composer');
    const [inputText, setInputText] = useState('');
    const count = Number(new URLSearchParams(location.search).get('history') || 0);
    const [messages, update] = useState(() => Array.from({ length: count }, (_, i) => message(i + 1, i % 2 ? 'response' : 'user', `历史消息 ${i + 1}。\n\n${'用于检查长历史的虚拟列表定位。'.repeat(i % 5 + 1)}`)));
    const [conversationId, switchConversation] = useState('1');
    const [historyLoaded, setHistoryLoaded] = useState(true);
    const [height, setHeight] = useState('100dvh');
    const [imageSrc, setImageSrc] = useState('');
    const [results, setResults] = useState<Result[]>([]);
    const [busy, setBusy] = useState(false);
    const [progress, setProgress] = useState('');
    const epoch = useRef(1);
    const profile = useRef<Result['commits']>([]);
    const scroll = useScrollManagement({ disableTailObservation: true });
    const send = () => {
        const id = ++serial;
        update(prev => [...prev.filter(m => m.message_type !== 'butler_processing'), message(id, 'user', inputText || `请帮我检查这次发送 ${id}`), message(++serial, 'butler_processing', '正在处理')]);
    };
    const sendFromComposer = () => { send(); setInputText(''); };
    const respond = () => update(prev => [...prev.filter(m => m.message_type !== 'butler_processing'), message(++serial, 'response', '好的，我已经收到，接下来继续处理。')]);
    async function runSuite() {
        setBusy(true); setResults([]);
        const output: Result[] = [];
        const availableScenarios = ['empty', 'short', 'long', 'rapid', 'instant-reply', 'delayed-reply',
            'wheel-synthetic', 'touch-synthetic', 'wheel-during-scroll', 'rapid-during-scroll', 'rapid-long', 'streaming-reply', 'input-collapse', 'late-image', 'resize', 'switch', 'async-history', 'delete-history', 'reduced-motion-emulated'];
        const params = new URLSearchParams(location.search);
        const selected = params.get('scenario');
        const repeats = Math.max(1, Math.min(10, Number(params.get('repeat')) || 1));
        const scenarios = Array.from({length:repeats}, () => availableScenarios.filter(s => !selected || s === selected)).flat();
        const realAnimate = Element.prototype.animate;
        const realMatchMedia = window.matchMedia;
        try {
            for (const scenario of scenarios) {
                setProgress(scenario);
                let animationCalls = 0;
                let expectedScroll: number | null = null;
                const base = history(scenario === 'empty' ? 0 : ['long','rapid-long'].includes(scenario) ? 120 : 8);
                flushSync(() => {
                    switchConversation(String(++epoch.current)); update(base); setHistoryLoaded(true);
                    setHeight(scenario === 'input-collapse' ? '80dvh' : '100dvh'); setImageSrc('');
                });
                await delay(700);
                const container = scroll.scrollContainerRef.current!;
                const samples: Frame[] = [];
                const viewport = container.getBoundingClientRect();
                const anchorId = [...container.querySelectorAll<HTMLElement>('[data-message-item]')].filter(el => {
                    if (el.parentElement?.closest('[data-message-item]')) return false;
                    const rect = el.getBoundingClientRect(); return rect.bottom > viewport.top && rect.top < viewport.bottom;
                }).at(-1)?.dataset.messageId;
                let actionMs = 0;
                let recording = true;
                profile.current = [];
                const started = performance.now();
                let historyCommitMs: number | null = null;
                const longTasks: {ms:number; duration:number}[] = [];
                const observer = PerformanceObserver.supportedEntryTypes.includes('longtask')
                    ? new PerformanceObserver(list => list.getEntries().forEach(entry => longTasks.push({ms:entry.startTime-started,duration:entry.duration}))) : null;
                observer?.observe({type:'longtask'});
                const collect = () => {
                    const row = [...container.querySelectorAll<HTMLElement>('[data-message-item]')]
                        .filter(el => el.textContent?.includes('请帮我检查')).at(-1);
                    const anchor = anchorId ? container.querySelector<HTMLElement>(`[data-message-item][data-message-id='${anchorId}']`) : null;
                    const origin = container.getBoundingClientRect().top;
                    samples.push({ phase: container.dataset.sendPhase ?? 'idle', clientHeight: container.clientHeight,
                        anchorTop: anchor ? anchor.getBoundingClientRect().top - origin : null,
                        bubbleTop: row?.lastElementChild ? row.lastElementChild.getBoundingClientRect().top - origin : null,
                        thumbPosition: container.scrollTop / Math.max(1, container.scrollHeight - container.clientHeight),
                        thumbSize: Math.min(1,container.clientHeight / Math.max(1,container.scrollHeight)), nodes: container.querySelectorAll('[data-message-item]').length, ms: performance.now() - started,
                        top: row ? row.getBoundingClientRect().top - container.getBoundingClientRect().top : null,
                        scroll: container.scrollTop, height: container.scrollHeight,
                        opacity: row?.lastElementChild ? Number(getComputedStyle(row.lastElementChild).opacity) : null });
                    if (recording) requestAnimationFrame(() => setTimeout(collect, 0));
                };
                Element.prototype.animate = function (...args) { animationCalls++; return realAnimate.apply(this, args); };
                if (scenario === 'reduced-motion-emulated') {
                    window.matchMedia = query => query === '(prefers-reduced-motion: reduce)'
                        ? new Proxy(realMatchMedia.call(window, query), { get(target, key) {
                            if (key === 'matches') return true;
                            const value = Reflect.get(target, key, target);
                            return typeof value === 'function' ? value.bind(target) : value;
                        } })
                        : realMatchMedia.call(window, query);
                }
                requestAnimationFrame(() => setTimeout(collect, 0));
                // Capture the old viewport before any message/layout mutation.
                while (samples.length < 5) await delay(16);
                actionMs = performance.now() - started;
                if (scenario === 'async-history') {
                    flushSync(() => { switchConversation(String(++epoch.current)); setHistoryLoaded(false); update([]); });
                    await delay(100);
                    const commitStarted = performance.now();
                    const applyHistory = () => { update(history(120)); setHistoryLoaded(true); };
                    if (params.get('flushHistory') === '1') {
                        flushSync(applyHistory);
                        historyCommitMs = performance.now() - commitStarted;
                    } else {
                        // Match ConversationUI's async response: React batches these setters.
                        // Setter duration is not commit duration; only record the latter in stress mode.
                        applyHistory();
                    }
                } else if (scenario === 'delete-history') {
                    container.dispatchEvent(new WheelEvent('wheel', { deltaY: -120 }));
                    container.scrollTop = 160;
                    flushSync(() => update(base.slice(0, -2)));
                    container.scrollTop = 160;
                    expectedScroll = container.scrollTop;
                } else {
                    flushSync(send);
                    if (params.has('fault')) {
                        await delay(params.get('fault') === 'reverse' ? 280 : 48);
                        const content = container.firstElementChild as HTMLElement;
                        content.style.translate = '0 80px';
                        await delay(80);
                        content.style.translate = '';
                    }
                    if (['rapid','rapid-long','rapid-during-scroll'].includes(scenario)) { await delay(scenario === 'rapid-during-scroll' ? 250 : 70); flushSync(send); }
                    if (scenario === 'input-collapse') flushSync(() => setHeight('100dvh'));
                    if (scenario === 'streaming-reply') {
                        flushSync(respond);
                        for (let i = 0; i < 8; i++) {
                            await delay(40);
                            flushSync(() => update(prev => prev.map((m, index) => index === prev.length - 1
                                ? {...m, content:m.content + ' 流式追加文字。'} : m)));
                        }
                    }
                    if (scenario === 'instant-reply') flushSync(respond);
                    if (scenario === 'delayed-reply') { await delay(650); flushSync(respond); }
                    if (['wheel-synthetic','touch-synthetic','wheel-during-scroll'].includes(scenario)) {
                        await delay(scenario === 'wheel-during-scroll' ? 250 : 90);
                        container.dispatchEvent(scenario !== 'touch-synthetic'
                            ? new WheelEvent('wheel', { deltaY: -120 }) : new Event('touchmove'));
                        container.scrollTop = Math.max(0, container.scrollTop - 120);
                        expectedScroll = container.scrollTop;
                    }
                    if (scenario === 'late-image') {
                        await delay(650);
                        flushSync(() => setImageSrc(new URLSearchParams(location.search).has('networkImage') ? '/send-motion-delayed.svg?run=' + epoch.current : 'data:image/svg+xml,' + encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="400" height="500"><rect width="400" height="500" fill="skyblue"/></svg>')));
                    }
                    if (scenario === 'resize') { await delay(120); flushSync(() => setHeight('70dvh')); }
                    if (scenario === 'switch') {
                        await delay(80);
                        flushSync(() => { switchConversation(String(++epoch.current)); update([]); });
                        await delay(100); flushSync(send);
                    }
                }
                await delay(1100);
                recording = false;
                await delay(40);
                Element.prototype.animate = realAnimate;
                window.matchMedia = realMatchMedia;
                observer?.disconnect();
                const img = container.querySelector('img[alt="迟加载测试图"]');
                const imageLoaded = img instanceof HTMLImageElement && img.complete && img.naturalWidth > 0;
                const last = samples.at(-1)!;
                const gaps = samples.slice(1).map((frame, i) => frame.ms - samples[i].ms).sort((a, b) => a - b);
                const scrollDrift = expectedScroll === null ? null : last.scroll - expectedScroll;
                const noEntry = scenario === 'async-history' || scenario === 'delete-history' || scenario === 'reduced-motion-emulated';
                const passed = (expectedScroll !== null ? Math.abs(scrollDrift!) <= 1
                    : scenario === 'async-history' ? Math.abs(container.scrollHeight - container.clientHeight - container.scrollTop) <= 1
                    : last.top !== null && Math.abs(last.top - 8) <= 1)
                    && (noEntry ? animationCalls === 0 : animationCalls > 0)
                    && (scenario !== 'late-image' || imageLoaded);
                const trajectory = analyzeTrajectory(samples, actionMs);
                output.push({ trajectory, scenario, actionMs, finalTop: last.top, scrollDrift, animationCalls,
                    frames: samples.length, maxFrameMs: Math.max(...gaps),
                    p95FrameMs: gaps[Math.floor(gaps.length * .95)] || 0, passed: passed && (scenario !== 'input-collapse' || (trajectory.entryScrollDrift! <= 1 && (trajectory.entryAnchorDrift ?? 0) <= 1 && trajectory.bubbleBeforeScroll)) && (!['short','long','empty','instant-reply','delayed-reply','rapid','rapid-long','streaming-reply'].includes(scenario) || trajectory.passed), samples, historyCommitMs, longTasks, imageLoaded, commits: [...profile.current] });
                setResults([...output]);
            }
        } catch (error) { setProgress(String(error)); }
        finally { Element.prototype.animate = realAnimate; window.matchMedia = realMatchMedia; setBusy(false); }
    }
    // Do not stringify every previous frame again on every send/stream update:
    // diagnostic rendering itself would contaminate the measured interaction.
    const summaryJson = useMemo(() => JSON.stringify(results.map(({samples,commits,...result}) => result),null,2), [results]);
    const samplesJson = useMemo(() => JSON.stringify(results, null, 2), [results]);
    function downloadResults() {
        const blob = new Blob([JSON.stringify({ userAgent: navigator.userAgent,
            devicePixelRatio: window.devicePixelRatio, buildMode: import.meta.env.MODE,
            historyUpdateMode: new URLSearchParams(location.search).get('flushHistory') === '1' ? 'flushSync-stress' : 'async-batched', results }, null, 2)], {type:'application/json'});
        const url = URL.createObjectURL(blob);
        const link = document.createElement('a');
        link.href = url; link.download = 'send-motion-results.json'; link.click();
        setTimeout(() => URL.revokeObjectURL(url), 1000);
    }
    return <main className="benchmark" style={{height}}>
        <header><strong>发送动画回归 · 合成数据</strong><button disabled={busy} onClick={send}>发送</button><button disabled={busy} onClick={respond}>回复</button><button disabled={busy} onClick={() => { switchConversation(String(Number(conversationId) + 1)); update([]); }}>切会话</button><button disabled={busy} onClick={() => void runSuite()}>运行覆盖矩阵</button><span role="status">{busy ? progress : `${results.filter(r => r.passed).length}/${results.length} 通过`}</span><button disabled={busy || !results.length} onClick={downloadResults}>下载结果</button></header>
        <div ref={scroll.scrollContainerRef} style={{overflowAnchor:'none'}} className="benchmark-scroll flex flex-col gap-4" data-aipp-slot="chat-conversation-scroll" onScroll={scroll.handleScroll} onWheel={scroll.handleUserScrollIntent} onTouchMove={scroll.handleUserScrollIntent}>
            <Profiler id="message-list" onRender={(_id, phase, actualDuration, _baseDuration, startTime, commitTime) => profile.current.push({phase, actualDuration, startTime, commitTime})}><VirtuosoMessageList historyLoaded={historyLoaded} inlineInteractionItems={imageSrc ? [{key:"late-image", content:<img alt="迟加载测试图" src={imageSrc} style={{width:120}} />}]:undefined} conversationId={conversationId} transcriptKey={conversationId}
                allDisplayMessages={messages} streamingMessages={map} shiningMessageIds={set}
                shiningMcpCallId={null} reasoningExpandStates={map} mcpToolCallStates={map}
                generationGroups={map} selectedVersions={map} getGenerationGroupControl={() => null}
                handleGenerationVersionChange={noop} onCodeRun={noop} onMessageRegenerate={noop}
                onMessageEdit={noop} onMessageFork={noop} onToggleReasoningExpand={noop}
                scrollContainerRef={scroll.scrollContainerRef} pendingScrollMessageId={null}
                clearPendingScrollMessageId={noop} setShiningMessageIds={noop} smartScroll={scroll.smartScroll}
                onScrollStateChange={scroll.syncScrollState} /></Profiler>
            <div ref={scroll.messagesEndRef} data-aipp-slot="chat-messages-end-anchor" />
        </div>
        {realComposer && <InputArea inputText={inputText} setInputText={setInputText} handleSend={sendFromComposer} aiIsResponsing={false} ariaLabel="实际输入组件" />}
        <pre data-testid="motion-summary" style={{maxHeight:160,overflow:"auto"}}>{summaryJson}</pre>
        <details style={{maxHeight:140,overflow:'auto'}}><summary>逐帧结果（合成事件；不是原生 WebView 性能）</summary>
            <pre data-testid="motion-results">{samplesJson}</pre></details>
    </main>;
}
createRoot(document.getElementById('root')!).render(<AntiLeakageProvider enabled={false}><Chat /></AntiLeakageProvider>);
