import { DatabaseSync } from 'node:sqlite';
import { mkdir, readFile, writeFile, unlink } from 'node:fs/promises';
import { homedir } from 'node:os';
import path from 'node:path';
import { createHash } from 'node:crypto';

const defaultData = process.platform === 'win32' ? process.env.APPDATA
    : process.platform === 'darwin' ? path.join(homedir(), 'Library/Application Support')
    : process.env.XDG_CONFIG_HOME || path.join(homedir(), '.config');
const dbDir = process.argv[2] || path.join(defaultData, 'com.xieisabug.aipp/db');
const output = path.resolve(process.argv[3] || 'tmp/chat-perf/fixtures');
const conversationDb = new DatabaseSync(path.join(dbDir, 'conversation.db'), { readOnly: true });
const mcpDb = new DatabaseSync(path.join(dbDir, 'mcp.db'), { readOnly: true });
try {
    conversationDb.exec('BEGIN');
    mcpDb.exec('BEGIN');
    const calls = mcpDb.prepare('SELECT * FROM mcp_tool_call ORDER BY id').all();
    const callsByConversation = Map.groupBy(calls, c => c.conversation_id);
    const stats = conversationDb.prepare(`SELECT c.id, c.name, COUNT(m.id) AS messages,
        COALESCE(SUM(length(m.content)),0) AS chars,
        MAX(length(m.content)) AS largestMessageChars,
        SUM(CASE WHEN m.message_type='tool_result' THEN length(m.content) ELSE 0 END) AS toolResultChars,
        SUM(CASE WHEN m.message_type='reasoning' THEN length(m.content) ELSE 0 END) AS reasoningChars,
        SUM(CASE WHEN instr(m.content,'data:image')>0 OR instr(m.content,'![')>0 THEN 1 ELSE 0 END) AS imageMessages,
        SUM(CASE WHEN instr(m.content,'|---')>0 OR instr(m.content,'| ---')>0 THEN 1 ELSE 0 END) AS tableMessages,
        SUM(CASE WHEN instr(m.content,'$$')>0 THEN 1 ELSE 0 END) AS mathMessages,
        SUM(CASE WHEN m.message_type NOT IN ('system','tool_result') THEN 1 ELSE 0 END) AS visibleMessages,
        COALESCE(SUM(CASE WHEN m.message_type NOT IN ('system','tool_result') THEN length(m.content) ELSE 0 END),0) AS visibleChars,
        COALESCE(SUM(CASE WHEN m.message_type NOT IN ('system','tool_result') THEN
            (length(m.content)-length(replace(m.content,char(96,96,96),'')))/3
            + (length(m.content)-length(replace(m.content,'~~~','')))/3 ELSE 0 END),0) AS visibleCodeFences,
        COALESCE(SUM((length(m.content)-length(replace(m.content, '~~~', '')))/3),0) AS tildeFences,
        COALESCE(SUM((length(m.content)-length(replace(m.content, char(96,96,96), '')))/3),0) AS backtickFences
        FROM conversation c JOIN message m ON m.conversation_id=c.id
        GROUP BY c.id HAVING COUNT(m.id)>1`).all().map(row => {
        const tools = callsByConversation.get(row.id) || [];
        return { ...row, tools: tools.length,
            previews: tools.filter(t => /preview_code/.test(t.tool_name)).length,
            failedTools: tools.filter(t => t.status === 'failed').length,
            scriptedPreviews: tools.filter(t => /preview_code/.test(t.tool_name) && /<script[\s>]/i.test(t.parameters)).length,
            toolChars: tools.reduce((n,t) => n + (t.parameters?.length || 0) + (t.result?.length || 0), 0),
            codeFences: row.backtickFences + row.tildeFences };
    });
    const selected = new Map();
    const metrics = ['previews', 'scriptedPreviews', 'tools', 'visibleMessages', 'visibleChars',
        'chars', 'largestMessageChars', 'visibleCodeFences', 'toolResultChars', 'reasoningChars',
        'imageMessages', 'tableMessages', 'mathMessages', 'failedTools'];
    for (const metric of metrics) {
        const top = [...stats].filter(s => s[metric] > 0)
            .sort((a,b) => b[metric]-a[metric] || a.id-b.id).slice(0,2);
        for (const item of top) {
            if (!selected.has(item.id)) selected.set(item.id, { ...item, selectedFor: [] });
            selected.get(item.id).selectedFor.push(metric);
        }
    }
    const longChat = stats.filter(s => s.visibleMessages >= 20).sort((a,b) => b.visibleChars-a.visibleChars).slice(0,2);
    for (const item of longChat) {
        if (!selected.has(item.id)) selected.set(item.id, { ...item, selectedFor: [] });
        selected.get(item.id).selectedFor.push('longChat');
    }
    const baseline = stats.filter(s => s.visibleMessages >= 10 && s.visibleMessages <= 40 && s.visibleChars >= 4000 && s.visibleChars < 30000)
        .sort((a,b) => a.chars-b.chars)[0];
    if (baseline && !selected.has(baseline.id)) selected.set(baseline.id, { ...baseline, selectedFor: ['baseline'] });
    await mkdir(output, { recursive: true });
    const oldManifest = await readFile(path.join(output, 'manifest.json'), 'utf8')
        .then(JSON.parse).catch(error => { if (error.code === 'ENOENT') return { cases: [] }; throw error; });
    const cases = [];
    for (const stats of selected.values()) {
        const conversation = conversationDb.prepare('SELECT * FROM conversation WHERE id=?').get(stats.id);
        const attachments = conversationDb.prepare(`SELECT a.* FROM message_attachment a
            JOIN message m ON m.id=a.message_id WHERE m.conversation_id=? ORDER BY a.id`).all(stats.id);
        const attachmentsByMessage = Map.groupBy(attachments, a => a.message_id);
        const messages = conversationDb.prepare('SELECT * FROM message WHERE conversation_id=? ORDER BY created_time,id').all(stats.id)
            .map(m => ({ ...m, regenerate: [], attachment_list: attachmentsByMessage.get(m.id) || [] }));
        const fixture = { schemaVersion: 1, conversation, messages, toolCalls: callsByConversation.get(stats.id) || [], stats };
        const data = JSON.stringify(fixture);
        const sha256 = createHash('sha256').update(data).digest('hex');
        const file = `conversation-${stats.id}.json`;
        await writeFile(path.join(output, file), data);
        cases.push({ ...stats, file, sha256, bytes: Buffer.byteLength(data) });
    }
    const manifest = { schemaVersion: 1, exportedAt: new Date().toISOString(), cases,
        limitations: ['Separate read snapshots for conversation.db and mcp.db; export while app is idle.',
            'Raw messages and attachment metadata preserved; external attachment files are not copied.',
            'Private chat content: local use only; do not commit or publish.'] };
    await writeFile(path.join(output, 'manifest.json'), JSON.stringify(manifest, null, 2));
    for (const oldCase of oldManifest.cases) {
        if (!/^conversation-\d+\.json$/.test(oldCase.file) || cases.some(c => c.file === oldCase.file)) continue;
        const stalePath = path.resolve(output, oldCase.file);
        if (path.dirname(stalePath) !== output) throw new Error('Fixture cleanup escaped output directory');
        await unlink(stalePath).catch(error => { if (error.code !== 'ENOENT') throw error; });
    }
    await writeFile(path.join(output, 'ranking.json'), JSON.stringify(stats.sort((a,b) => b.chars-a.chars), null, 2));
    console.table(cases.map(({id,messages,chars,tools,previews,codeFences,selectedFor}) =>
        ({ id,messages,chars,tools,previews,codeFences,selectedFor:selectedFor.join(',') })));
    console.log(`Exported ${cases.length} cases to ${output}`);
} finally {
    conversationDb.close();
    mcpDb.close();
}
