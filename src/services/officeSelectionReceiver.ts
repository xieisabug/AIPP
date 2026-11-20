/**
 * “让 AI 修改”的接收方：每个显示会话的 ConversationUI 注册自己，Office 预览窗口把用户的修改要求
 * 连同选区作为一条消息发到对应会话。预览没有指明会话时，发到本窗口最近注册（正在显示）的会话。
 */
export interface OfficeSelectionReceiver {
    /** 还没创建的新会话为 null。 */
    conversationId: number | null;
    /** 显示给用户确认：发到哪里。 */
    label: string;
    send: (prompt: string) => Promise<void>;
}

const receivers: OfficeSelectionReceiver[] = [];
const listeners = new Set<() => void>();

function notify() {
    listeners.forEach((listener) => listener());
}

export function registerOfficeSelectionReceiver(receiver: OfficeSelectionReceiver): () => void {
    receivers.push(receiver);
    notify();
    return () => {
        const index = receivers.lastIndexOf(receiver);
        if (index >= 0) receivers.splice(index, 1);
        notify();
    };
}

export function findOfficeSelectionReceiver(conversationId?: number | null): OfficeSelectionReceiver | null {
    if (conversationId != null) {
        for (let i = receivers.length - 1; i >= 0; i -= 1) {
            if (receivers[i].conversationId === conversationId) return receivers[i];
        }
        return null;
    }
    return receivers.length ? receivers[receivers.length - 1] : null;
}

export function subscribeOfficeSelectionReceivers(listener: () => void): () => void {
    listeners.add(listener);
    return () => listeners.delete(listener);
}

/** 仅供测试。 */
export function __resetOfficeSelectionReceiversForTests(): void {
    receivers.splice(0, receivers.length);
    listeners.clear();
}
