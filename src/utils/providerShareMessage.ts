export function providerShareMessage(name: string, code: string, password?: string) {
    return [
        `我给你分享了一个 AIPP 模型配置「${name}」。`,
        `分享码：${code}`,
        password ? `密码：${password}` : "密码我会另外告诉你",
        "在 AIPP 首次引导里选「有人发给我分享码」，或者在 设置 → 模型提供商 → 导入 里，整段粘贴进去就行。",
    ].join("\n");
}

export function parseProviderShareMessage(text: string) {
    const candidates = (text.match(/[A-Za-z0-9+/=]{40,}/g) ?? []).sort((a, b) => b.length - a.length);
    for (const code of candidates) {
        try {
            const envelope = JSON.parse(atob(code));
            if (envelope.type !== "provider" || !envelope.encrypted_data || !envelope.nonce) continue;
            const password = /(?:密码|\bpassword)\s*[:：][ \t]*([^\r\n]*)/i.exec(text)?.[1].trim();
            return { code, password: password || undefined };
        } catch {
            /* 其他文本里的长串不属于 AIPP 分享码。 */
        }
    }
    return null;
}
