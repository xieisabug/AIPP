// Synthetic send harness: real list, bubbles, virtualization and viewport sizing;
// no backend/model calls. Open the production-built send-motion.html.
import { useState } from 'react';
import { createRoot } from 'react-dom/client';
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
function Chat() {
    const count = Number(new URLSearchParams(location.search).get('history') || 0);
    const [messages, update] = useState(() => Array.from({ length: count }, (_, i) => message(i + 1, i % 2 ? 'response' : 'user', `历史消息 ${i + 1}。\n\n${'用于检查长历史的虚拟列表定位。'.repeat(i % 5 + 1)}`)));
    const [conversationId, switchConversation] = useState('1');
    const scroll = useScrollManagement({ disableTailObservation: true });
    const send = () => {
        const id = ++serial;
        update(prev => [...prev.filter(m => m.message_type !== 'butler_processing'), message(id, 'user', `请帮我检查这次发送 ${id}`), message(++serial, 'butler_processing', '正在处理')]);
    };
    const respond = () => update(prev => [...prev.filter(m => m.message_type !== 'butler_processing'), message(++serial, 'response', '好的，我已经收到，接下来继续处理。')]);
    return <main className="benchmark">
        <header><strong>发送动画回归 · 合成数据</strong><button onClick={send}>发送</button><button onClick={respond}>回复</button><button onClick={() => { switchConversation(String(Number(conversationId) + 1)); update([]); }}>切会话</button></header>
        <div ref={scroll.scrollContainerRef} className="benchmark-scroll flex flex-col gap-4" data-aipp-slot="chat-conversation-scroll" onScroll={scroll.handleScroll} onWheel={scroll.handleUserScrollIntent} onTouchMove={scroll.handleUserScrollIntent}>
            <VirtuosoMessageList historyLoaded conversationId={conversationId} transcriptKey={conversationId}
                allDisplayMessages={messages} streamingMessages={map} shiningMessageIds={set}
                shiningMcpCallId={null} reasoningExpandStates={map} mcpToolCallStates={map}
                generationGroups={map} selectedVersions={map} getGenerationGroupControl={() => null}
                handleGenerationVersionChange={noop} onCodeRun={noop} onMessageRegenerate={noop}
                onMessageEdit={noop} onMessageFork={noop} onToggleReasoningExpand={noop}
                scrollContainerRef={scroll.scrollContainerRef} pendingScrollMessageId={null}
                clearPendingScrollMessageId={noop} setShiningMessageIds={noop} smartScroll={scroll.smartScroll}
                onScrollStateChange={scroll.syncScrollState} />
            <div ref={scroll.messagesEndRef} data-aipp-slot="chat-messages-end-anchor" />
        </div>
    </main>;
}
createRoot(document.getElementById('root')!).render(<AntiLeakageProvider enabled={false}><Chat /></AntiLeakageProvider>);
