import { randomBytes } from 'node:crypto';
import { deadline, launch, splitModel, stopChild } from './common.mjs';

export async function openOpenCode(config, emit, dependencies = {}) {
  const fetch = dependencies.fetch || globalThis.fetch;
  const password = randomBytes(24).toString('hex');
  const nativeConfig = process.env.OPENCODE_CONFIG_CONTENT ? JSON.parse(process.env.OPENCODE_CONFIG_CONTENT) : {};
  if (config.mcp) nativeConfig.mcp = { ...nativeConfig.mcp, aipp: { type: 'local', command: [config.mcp.command, ...config.mcp.args], environment: config.mcp.env, enabled: true } };
  const child = (dependencies.launch || launch)(config.command || 'opencode', ['serve', '--hostname=127.0.0.1', '--port=0'], {
    cwd: config.cwd,
    env: { ...process.env, OPENCODE_SERVER_PASSWORD: password,
      OPENCODE_CONFIG_CONTENT: JSON.stringify(nativeConfig) },
  });
  const abort = new AbortController();
  let closing = false, turn = null, session, model = config.model, errorMessage, sawActivity = false;
  child.on('error', error => emit({ type: 'fatal', error: error.message }));
  child.on('exit', (code, signal) => {
    if (!closing) emit({ type: 'fatal', error: `OpenCode server exited (code=${code}, signal=${signal})` });
  });
  child.stderr.on('data', data => process.stderr.write(data));
  const close = async () => { closing = true; abort.abort(); await stopChild(child); };
  try {
    const base = await deadline(new Promise((resolve, reject) => {
      let output = '';
      child.stdout.on('data', chunk => {
        output = (output + chunk).slice(-16384);
        const match = output.match(/server listening on\s+(http:\/\/127\.0\.0\.1:\d+)/i);
        if (match) resolve(match[1]);
      });
      child.once('error', reject);
      child.once('exit', (code, signal) => reject(new Error(`OpenCode startup exited (${code}, ${signal})`)));
    }), 'OpenCode server startup');
    const headers = { Authorization: `Basic ${Buffer.from(`opencode:${password}`).toString('base64')}`,
      'Content-Type': 'application/json', 'x-opencode-directory': encodeURIComponent(config.cwd) };
    const api = async (path, method = 'GET', body) => {
      const response = await fetch(`${base}${path}`, { method, headers,
        ...(body === undefined ? {} : { body: JSON.stringify(body) }), signal: AbortSignal.timeout(30000) });
      const text = await response.text();
      if (!response.ok) throw new Error(`OpenCode ${method} ${path} HTTP ${response.status}: ${text}`);
      return text ? JSON.parse(text) : null;
    };
    await api('/global/health');
    const models = async () => {
      const catalog = await api('/provider');
      return catalog.all.filter(p => catalog.connected.includes(p.id)).flatMap(p =>
        Object.values(p.models).map(m => ({ id: `${p.id}/${m.id}`, name: m.name || m.id })));
    };
    if (config.probe) return { models, close };
    const data = config.session ? await api(`/session/${encodeURIComponent(config.session)}`) : await api('/session', 'POST', {});
    session = data.id;
    if (!session || (config.session && session !== config.session)) throw new Error('OpenCode returned an invalid or different session ID');
    // Establish event subscription before admitting any prompts.
    const response = await fetch(`${base}/event`, { headers, signal: abort.signal });
    if (!response.ok || !response.body) throw new Error(`OpenCode event subscription HTTP ${response.status}`);
    const parts = new Map();
    const partTypes = new Map();
    const roles = new Map();
    const pendingPermissions = new Set();
    const questions = new Map();
    const waitingParts = new Map();
    let waitingBytes = 0, pendingIdle = false;
    const processEvent = event => {
      const p = event.properties || {};
      const sid = p.sessionID || p.part?.sessionID || p.info?.sessionID;
      if (sid !== session || !turn) return;
      const messageID = p.part?.messageID || p.messageID;
      if (['message.part.updated', 'message.part.delta'].includes(event.type) && !roles.has(messageID)) {
        const size = JSON.stringify(event).length;
        if (waitingBytes + size > 2 * 1024 * 1024) throw new Error('OpenCode parts awaiting message identity exceeded 2 MiB');
        const pending = waitingParts.get(messageID) || [];
        pending.push({ event, size }); waitingParts.set(messageID, pending); waitingBytes += size;
        return;
      }
      if (event.type === 'message.updated') {
        roles.set(p.info.id, p.info.role);
        const pending = waitingParts.get(p.info.id) || [];
        waitingParts.delete(p.info.id);
        for (const item of pending) { waitingBytes -= item.size; processEvent(item.event); }
        if (pendingIdle && waitingParts.size === 0) processEvent({ type: 'session.idle', properties: { sessionID: session } });
      } else if (event.type === 'session.status' && p.status?.type === 'busy') {
        sawActivity = true;
      } else if (event.type === 'message.part.updated') {
        const part = p.part;
        if (roles.get(part.messageID) !== 'assistant') return;
        sawActivity = true;
        partTypes.set(part.id, part.type);
        if (part.type === 'text') {
          const previous = parts.get(part.id) || '';
          if (part.text?.startsWith(previous) && part.text.length > previous.length) emit({ type: 'text', turn, text: part.text.slice(previous.length) });
          parts.set(part.id, part.text || '');
        } else if (part.type === 'reasoning') {
          parts.set(part.id, part.text || '');
          emit({ type: 'activity', turn, id: part.id, kind: 'reasoning', status: part.time?.end ? 'success' : 'running', output: part.text || '' });
        } else if (part.type === 'tool') {
          const state = part.state || {};
          const interrupted = state.metadata?.interrupted === true || ['Interrupted', 'Tool execution aborted', 'Task cancelled'].includes(state.error);
          const badExit = typeof state.metadata?.exit === 'number' && state.metadata.exit !== 0;
          const status = interrupted ? 'cancelled' : state.status === 'error' || badExit ? 'failed' : state.status === 'completed' ? 'success' : state.status === 'pending' ? 'pending' : 'running';
          emit({ type: 'activity', turn, id: part.callID, kind: 'tool', title: part.tool, status,
            input: state.input, output: state.output, error: state.error || (badExit ? `Process exited with code ${state.metadata.exit}` : undefined) });
        }
      } else if (event.type === 'message.part.delta' && p.field === 'text') {
        if (roles.get(p.messageID) !== 'assistant') return;
        sawActivity = true;
        parts.set(p.partID, (parts.get(p.partID) || '') + p.delta);
        if (partTypes.get(p.partID) === 'reasoning') {
          emit({ type: 'activity', turn, id: p.partID, kind: 'reasoning', status: 'running', output: p.delta, append: true });
        } else if (partTypes.get(p.partID) === 'text') emit({ type: 'text', turn, text: p.delta });
      } else if (event.type === 'permission.asked') {
        pendingPermissions.add(p.id);
        emit({ type: 'permission', turn, id: p.id, title: p.permission, input: p,
          options: [{ id: 'once', name: '本次允许', kind: 'allow_once' },
            { id: 'always', name: '会话内允许', kind: 'allow_always' }, { id: 'reject', name: '拒绝', kind: 'reject_once' }] });
      } else if (event.type === 'question.asked') {
        if (new Set(p.questions.map(question => question.question)).size !== p.questions.length) {
          throw new Error('OpenCode question contains duplicate question text that AIPP cannot address independently');
        }
        questions.set(p.id, p.questions);
        emit({ type: 'question', turn, id: p.id, questions: p.questions.map((question, index) => ({
          question: question.question, header: `question_${index}`, options: question.options,
          multiSelect: question.multiple === true,
        })) });
      } else if (event.type === 'session.error') {
        errorMessage = JSON.stringify(p.error || { message: 'OpenCode session.error omitted error details' });
        emit({ type: 'done', turn, error: errorMessage }); turn = null;
        pendingPermissions.clear(); questions.clear();
      }
      else if (sawActivity && (event.type === 'session.idle' || (event.type === 'session.status' && p.status?.type === 'idle'))) {
        if (waitingParts.size) { pendingIdle = true; return; }
        emit({ type: 'done', turn, error: errorMessage }); turn = null; pendingPermissions.clear(); questions.clear();
      }
    };
    const consume = async () => {
      let buffer = '';
      const decoder = new TextDecoder();
      for await (const chunk of response.body) {
        buffer += decoder.decode(chunk, { stream: true });
        let end;
        while ((end = buffer.indexOf('\n')) >= 0) {
          const line = buffer.slice(0, end).trimEnd(); buffer = buffer.slice(end + 1);
          if (!line.startsWith('data:')) continue;
          const event = JSON.parse(line.slice(5));
          processEvent(event);
        }
      }
      if (!closing) throw new Error('OpenCode event stream ended unexpectedly');
    };
    void consume().catch(error => { if (!closing) emit({ type: 'fatal', error: error.message }); });
    return {
      session,
      async prompt(text, id) {
        if (turn) throw new Error('OpenCode already has an active turn');
        turn = id; errorMessage = undefined; sawActivity = false; pendingIdle = false;
        parts.clear(); partTypes.clear(); roles.clear(); waitingParts.clear(); waitingBytes = 0;
        await api(`/session/${session}/prompt_async`, 'POST', {
          ...(model ? { model: splitModel(model) } : {}), parts: [{ type: 'text', text }],
        });
      },
      async cancel() { if (turn) await api(`/session/${session}/abort`, 'POST'); },
      async reply(id, value) {
        if (!pendingPermissions.has(id)) throw new Error('Unknown or expired OpenCode permission request');
        if (!['once', 'always', 'reject'].includes(value)) throw new Error('Invalid OpenCode permission reply');
        pendingPermissions.delete(id);
        await api(`/permission/${encodeURIComponent(id)}/reply`, 'POST', { reply: value });
      },
      async answer(id, answers) {
        const items = questions.get(id);
        if (!items) throw new Error('Unknown or expired OpenCode question');
        if (!answers) { questions.delete(id); await api(`/question/${encodeURIComponent(id)}/reject`, 'POST'); return; }
        const values = items.map((question, index) => {
          const value = answers[question.question];
          if (typeof value !== 'string') throw new Error(`Missing answer for OpenCode question ${index}`);
          // AIPP's card returns joined labels. Preserve commas inside an option;
          // reject an ambiguous encoding instead of silently changing selections.
          const labels = question.options.map(option => option.label);
          const decode = (remaining, used = []) => {
            const matches = labels.filter(label => !used.includes(label) && (remaining === label || remaining.startsWith(`${label}, `)));
            if (matches.length > 1) throw new Error('Ambiguous OpenCode multi-select answer; option labels overlap');
            if (!matches.length) return remaining ? [remaining] : [];
            const label = matches[0];
            return [label, ...decode(remaining.slice(label.length + 2), [...used, label])];
          };
          const selected = question.multiple ? decode(value) : [value];
          if (question.custom === false && selected.some(item => !labels.includes(item))) {
            throw new Error(`OpenCode question ${index} does not allow a custom answer`);
          }
          return selected;
        });
        questions.delete(id);
        await api(`/question/${encodeURIComponent(id)}/reply`, 'POST', { answers: values });
      },
      async configure(key, value) {
        if (turn) throw new Error('Cannot change OpenCode configuration during a turn');
        if (key !== 'model') throw new Error(`Unsupported OpenCode configuration: ${key}`);
        splitModel(value); model = value;
      },
      models,
      close,
    };
  } catch (error) { await close(); throw error; }
}
