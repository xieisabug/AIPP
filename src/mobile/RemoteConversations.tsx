import { useCallback, useEffect, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ChevronLeft, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import VirtuosoMessageList from "@/components/conversation/VirtuosoMessageList";
import MessageItem from "@/components/MessageItem";
import { useScrollManagement } from "@/hooks/useScrollManagement";
import type { Message } from "@/data/Conversation";
import type { PairedHost } from "./MobileEnvironmentChat";
import { registerMobileBackHandler } from "./navigation";
import RemoteComposer, { chatAction } from "./RemoteComposer";

interface Conversation { id: number; name: string; kind: "normal" | "butler_main" | "butler_contact"; assistantId: number | null }
interface Directory { protocolVersion: number; capabilities: string[]; conversations: Conversation[]; butler: boolean; assistants?: {id:number;name:string}[] }
interface RemoteMessage { id: number; role: string; content: string }
const empty = new Map();
const noop = () => {};
const row = (message: Message) => <MessageItem remoteTextOnly message={message} />;

function Transcript({ host, conversation, writable, onAccepted }: { host: PairedHost; conversation: Conversation; writable: boolean; onAccepted: (id:number) => void }) {
    const [{messages,more}, setPage] = useState<{messages:Message[];more:boolean}>({messages:[],more:false});
    const [error, setError] = useState("");
    const [busy, setBusy] = useState(false);
    const [loaded, setLoaded] = useState(false);
    const [attempt, setAttempt] = useState(0);
    const [running,setRunning] = useState(false);
    const [progress,setProgress] = useState("");
    const [shining, setShining] = useState(new Set<number>());
    const { scrollContainerRef, messagesEndRef, smartScroll, handleUserScrollIntent, syncScrollState } = useScrollManagement();
    const key = `${host.id}/${conversation.id}`;
    const read = useCallback(async (before?: number, watch = false) => {
        const result = await invoke<{ messages: RemoteMessage[]; hasMore: boolean; running?:boolean; phase?:string; progress?:string }>("bridge_connect",
            { id: host.id, conversation: conversation.id, before, ...(watch ? {watch:true} : {}) });
        const messages = result.messages.map((message): Message => ({
            id: message.id, conversation_id: conversation.id, message_type: message.role, content: message.content,
            created_time: new Date(0), start_time: null, finish_time: null, llm_model_id: null,
            token_count: 0, input_token_count: 0, output_token_count: 0, regenerate: null,
        }));
        return { messages, hasMore: result.hasMore, running:result.running, progress:result.progress ||
            (result.phase === "mcp_executing" ? "电脑正在执行工具" : result.running ? "电脑正在回复…" : "") };
    }, [host.id, conversation.id]);
    useEffect(() => {
        let live = true;
        if (conversation.id < 0) { setLoaded(true); return; }
        let timer: ReturnType<typeof setTimeout>;
        async function watch() {
            if (!live) return;
            if (document.hidden) { timer = setTimeout(() => void watch(),1000); return; }
            try {
                const page = await read(undefined,true);
                if (!live) return;
                setPage(old => {
                    const byId = new Map(old.messages.map(m => [m.id,m]));
                    const latest = page.messages.map(m => {
                        const saved = byId.get(m.id);
                        return saved?.content === m.content && saved.message_type === m.message_type ? saved : m;
                    });
                    if (!page.hasMore || !latest.length || !byId.has(latest[0].id)) return {messages:latest,more:page.hasMore};
                    return {messages:[...old.messages.filter(m => m.id < latest[0].id), ...latest],more:old.more};
                });
                setRunning(!!page.running); setProgress(page.progress); setError(""); setLoaded(true);
            } catch (e) { if (live) setError(String(e)); }
            if (live) timer = setTimeout(() => void watch(),1000);
        }
        setBusy(true); setError("");
        read().then(page => {
            if (live) { setPage({messages:page.messages,more:page.hasMore}); setLoaded(true); }
        }).catch(e => { if (live) setError(String(e)); }).finally(() => {
            if (live) { setBusy(false); if (writable) void watch(); }
        });
        return () => { live = false; clearTimeout(timer); };
    }, [read, attempt, writable, conversation.id]);
    async function earlier() {
        setBusy(true); setError("");
        try {
            const page = await read(messages[0].id);
            setPage(previous => ({messages:[...page.messages.filter(m => !previous.messages.some(p => p.id === m.id)), ...previous.messages],more:page.hasMore}));
        } catch (e) { setError(String(e)); } finally { setBusy(false); }
    }
    return <>
        {error && <p role="alert" className="px-4 py-1 text-sm text-destructive">{error}</p>}
        <div className="flex items-center justify-between px-3">
            {more && <Button disabled={busy} variant="ghost" onClick={() => void earlier()}>较早消息</Button>}
            {conversation.id > 0 && <Button variant="ghost" disabled={busy} onClick={() => setAttempt(n => n + 1)}>刷新对话</Button>}
        </div>
        {loaded && !messages.length && <p className="p-4 text-sm text-muted-foreground">这条会话还没有可显示的消息。</p>}
        <div className="conversation-scroll-transparent-track flex min-h-0 flex-1 flex-col overflow-y-auto p-3"
            ref={scrollContainerRef} onTouchMoveCapture={handleUserScrollIntent} onWheelCapture={handleUserScrollIntent}>
            <VirtuosoMessageList conversationId={key} transcriptKey={key} historyLoaded={loaded} allDisplayMessages={messages}
                isMobile streamingMessages={empty} shiningMessageIds={shining} shiningMcpCallId={null} reasoningExpandStates={empty}
                mcpToolCallStates={empty} generationGroups={empty} selectedVersions={empty} getGenerationGroupControl={noop}
                handleGenerationVersionChange={noop} onCodeRun={noop} onMessageRegenerate={noop} onMessageEdit={noop} onMessageFork={noop}
                onToggleReasoningExpand={noop} mergeAssistantMessages={false} renderMessageRow={row} scrollContainerRef={scrollContainerRef}
                pendingScrollMessageId={null} clearPendingScrollMessageId={noop} setShiningMessageIds={setShining}
                onScrollStateChange={syncScrollState} smartScroll={smartScroll} />
            <div ref={messagesEndRef} />
        </div>
        {progress && <p role="status" className="px-3 text-sm text-muted-foreground">{progress}</p>}
        {writable && <RemoteComposer hostId={host.id} target={conversation} running={running} onAccepted={id => {
            if (id !== conversation.id) onAccepted(id); else setAttempt(n => n + 1);
        }} />}
    </>;
}

export default function RemoteConversations({ host, switcher }: { host: PairedHost; switcher: ReactNode }) {
    const location = `aipp.mobile.host.${host.id}`;
    const [selected, setSelected] = useState(() => Number(localStorage.getItem(location)) || null);
    const [mode, setMode] = useState(() => localStorage.getItem(location + ".mode") === "butler" ? "butler" : "chat");
    const [directory, setDirectory] = useState<Directory>();
    const [error, setError] = useState("");
    const [attempt, setAttempt] = useState(0);
    const [loading, setLoading] = useState(true);
    useEffect(() => {
        let live = true;
        setLoading(true); setError("");
        invoke<Directory>("bridge_connect", { id: host.id }).then(data => {
            if (data.protocolVersion !== 1 || !data.capabilities.includes("conversation_history")) throw new Error("电脑不支持此版本的对话历史协议");
            if (live) setDirectory(data);
        }).catch(e => { if (live) { setDirectory(undefined); setError(String(e)); } }).finally(() => { if (live) setLoading(false); });
        return () => { live = false; };
    }, [host, attempt]);
    const writable = !!directory?.capabilities.includes("chat");
    const newAssistant = writable && selected && selected < 0 ? directory?.assistants?.find(a => a.id === -selected) : undefined;
    const conversation: Conversation | undefined = newAssistant ? {id:-newAssistant.id,name:newAssistant.name,assistantId:newAssistant.id,kind:"normal"} :
        directory?.conversations.find(item => item.id === selected);
    const select = useCallback((id: number | null) => {
        if (id === null) localStorage.removeItem(location); else localStorage.setItem(location, String(id));
        setSelected(id);
    }, [location]);
    useEffect(() => registerMobileBackHandler("chat_ui", () => {
        if (!selected) return false;
        select(null); return true;
    }), [selected, select]);
    const entries = directory?.conversations.filter(item => mode === "chat" ? item.kind === "normal" : item.kind !== "normal");
    const [opening,setOpening] = useState(false);
    function openConversation(id:number) {
        select(id); setAttempt(n => n + 1);
    }
    async function openButler(assistant:number | null) {
        setOpening(true); setError("");
        try { const reply = await chatAction<{conversation:number}>(host.id,{action:"open_butler",assistant}); openConversation(reply.conversation); }
        catch (e) { setError(String(e)); } finally { setOpening(false); }
    }
    return <section className="safe-top safe-bottom flex h-full min-h-0 flex-col bg-background">
        <header className="flex min-h-14 items-center gap-1 border-b bg-secondary px-2">
            {conversation && <Button variant="ghost" size="icon" className="min-h-11 min-w-11" aria-label="返回会话列表" onClick={() => select(null)}><ChevronLeft /></Button>}
            <h1 className="min-w-0 flex-1 truncate text-center text-sm font-medium">{conversation?.name ?? "会话"}</h1>{switcher}
        </header>
        {error && <p role="alert" className="p-3 text-sm text-destructive">{error}</p>}
        {conversation ? <Transcript key={conversation.id} host={host} conversation={conversation} writable={writable} onAccepted={openConversation} /> :
            <div className="min-h-0 flex-1 overflow-auto p-3">
                <div className="mb-2 flex items-center gap-2">
                    {(["chat", "butler"] as const).map(item => <Button key={item} variant={mode === item ? "secondary" : "ghost"}
                        aria-pressed={mode === item} onClick={() => { setMode(item); localStorage.setItem(location + ".mode", item); }}>
                        {item === "chat" ? "聊天" : "管家"}</Button>)}
                    <Button className="ml-auto min-h-11 min-w-11" aria-label="重新连接电脑" variant="ghost" size="icon"
                        disabled={loading} onClick={() => setAttempt(n => n + 1)}><RefreshCw /></Button>
                </div>
                {loading && <p role="status">正在连接 {host.name}…</p>}
                {directory && !writable && <p className="mb-2 text-sm text-muted-foreground">当前仅可查看历史。要发送消息，请在电脑的设备互联设置中允许对话，然后重新连接。</p>}
                {writable && mode === "chat" && directory?.assistants?.map(a =>
                    <Button key={a.id} variant="outline" className="mr-2 mb-2 min-h-11" onClick={() => select(-a.id)}>与{a.name}新建对话</Button>)}
                {writable && mode === "butler" && directory?.butler && <>
                    <Button disabled={opening} variant="outline" className="mr-2 mb-2 min-h-11" onClick={() => void openButler(null)}>总管家</Button>
                    {directory.assistants?.map(a => <Button disabled={opening} key={a.id} variant="outline" className="mr-2 mb-2 min-h-11"
                        onClick={() => void openButler(a.id)}>{a.name}私聊</Button>)}
                </>}
                {!loading && !error && !entries?.length && <p className="text-muted-foreground text-sm">暂无已授权的会话。</p>}
                {entries?.map(item => <Button key={item.id} className="flex min-h-11 w-full justify-start" variant="ghost" onClick={() => select(item.id)}>
                    <span className="truncate">{item.name}</span>
                </Button>)}
            </div>}
    </section>;
}
