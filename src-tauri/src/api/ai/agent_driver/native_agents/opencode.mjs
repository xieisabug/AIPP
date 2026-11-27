import { pathToFileURL } from 'node:url';
import { basename } from 'node:path';
import { randomBytes } from 'node:crypto';
import { isDeepStrictEqual } from 'node:util';
import { setTimeout as delay } from 'node:timers/promises';
import { RequestRejected, deadline, launch, permissionOption, splitModel, stopChild } from './common.mjs';

export async function openOpenCode(config, emit, dependencies = {}) {
  const fetch = dependencies.fetch || globalThis.fetch;
  const password = randomBytes(24).toString('hex');
  const nativeConfig = process.env.OPENCODE_CONFIG_CONTENT ? JSON.parse(process.env.OPENCODE_CONFIG_CONTENT) : {};
  const rule = config.opencode_permission ?? 'default';
  if (!['default', 'ask', 'allow', 'deny'].includes(rule)) throw new Error(`Unsupported OpenCode permission: ${rule}`);
  const env = { ...process.env, OPENCODE_SERVER_PASSWORD: password };
  if (rule !== 'default') {
    const inherited = config.textOnly ? {} : JSON.parse(env.OPENCODE_PERMISSION || '{}');
    env.OPENCODE_PERMISSION = JSON.stringify({ '*': rule, ...(inherited && typeof inherited === 'object'
      ? Object.fromEntries(Object.entries(inherited).filter(([key]) => key !== '*')) : {}) });
  }
  if (config.mcp) nativeConfig.mcp = { ...nativeConfig.mcp, aipp: { type: 'local', command: [config.mcp.command, ...config.mcp.args], environment: config.mcp.env, enabled: true,
    ...(config.mcpAnswerTimeoutMs ? { timeout: config.mcpAnswerTimeoutMs } : {}) } };
  const child = (dependencies.launch || launch)(config.command || 'opencode', ['serve', '--hostname=127.0.0.1', '--port=0'], {
    cwd: config.cwd,
    env: { ...env, OPENCODE_CONFIG_CONTENT: JSON.stringify(nativeConfig) },
  });
  const abort = new AbortController();
  let closing = false, turn = null, session, model = config.model, errorMessage, cancelling = false, sawActivity = false;
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
    const api = async (path, method = 'GET', body, timeout = 30000) => {
      const response = await fetch(`${base}${path}`, { method, headers,
        ...(body === undefined ? {} : { body: JSON.stringify(body) }), signal: AbortSignal.timeout(timeout) });
      const text = await response.text();
      if (!response.ok) throw new Error(`OpenCode ${method} ${path} HTTP ${response.status}: ${text}`);
      return text ? JSON.parse(text) : null;
    };
    await api('/global/health');
    if (rule !== 'default') {
      const effective = (await api('/config')).permission;
      if ((typeof effective === 'string' ? effective : effective?.['*']) !== rule) {
        throw new Error(`OpenCode did not apply the requested default permission rule: ${rule}`);
      }
    }
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
    const partOwners = new Map();
    const roles = new Map();
    const pendingPermissions = new Set();
    const questions = new Map();
    const waitingParts = new Map();
    let waitingBytes = 0, pendingIdle = false;
    let previousTurn, wakes = 0;
    const emitPermission = async p => {
      const requestedTurn = turn;
      const current = () => turn === requestedTurn && !cancelling && !closing && pendingPermissions.has(p.id);
      let input = p, toolMeta;
      const { messageID, callID } = p.tool || {};
      if (config.mcp && typeof p.permission === 'string' && /^aipp_aipp_t\d+_[\w-]+$/.test(p.permission)
        && typeof messageID === 'string' && typeof callID === 'string') {
        try {
          const read = path => api(path, 'GET', undefined, 1000);
          const [settings, servers] = await Promise.all([read('/config'), read('/mcp')]);
          const source = settings.mcp?.aipp;
          // OpenCode flattens server and tool names. Another configured server may have the same prefix.
          const owners = Object.keys(servers).filter(key => p.permission.startsWith(key.replace(/[^a-zA-Z0-9_-]/g, '_') + '_'));
          if (owners.length === 1 && owners[0] === 'aipp' && servers.aipp.status === 'connected'
            && source?.type === 'local' && isDeepStrictEqual(source.command, nativeConfig.mcp.aipp.command)
            && isDeepStrictEqual(source.environment ?? {}, config.mcp.env ?? {})
            && !settings.plugin?.length) {
            // A before-tool plugin may rewrite the model's arguments. Only use an unmodified running call.
            for (const wait of [0, 75, 250]) {
              if (wait) await delay(wait);
              if (!current()) return;
              const msg = await read(`/session/${encodeURIComponent(session)}/message/${encodeURIComponent(messageID)}`);
              if (msg.info?.id !== messageID || msg.info.sessionID !== session || msg.info.role !== 'assistant') break;
              const calls = msg.parts.filter(part => part.type === 'tool' && part.callID === callID);
              if (calls.length > 1) break;
              const part = calls[0];
              if (!part || part.state.status === 'pending' || !roles.has(messageID)) continue;
              if (part.sessionID === session && part.messageID === messageID && roles.get(messageID) === 'assistant'
                && part.tool === p.permission && part.state.status === 'running'
                && part.state.input && typeof part.state.input === 'object' && !Array.isArray(part.state.input)) {
                input = part.state.input;
                toolMeta = { mcp: { serverName: 'aipp', toolName: part.tool.slice('aipp_'.length) } };
              }
              break;
            }
          }
        } catch { /* Missing or unsupported evidence keeps the normal permission flow. */ }
      }
      if (current()) emit({ type: 'permission', turn, id: p.id, title: p.permission, input, toolMeta,
        options: [{ id: 'once', name: '本次允许', kind: 'allow_once' },
          { id: 'always', name: '会话内允许', kind: 'allow_always' }, { id: 'reject', name: '拒绝', kind: 'reject_once' }] });
    };
    const startTurn = id => {
      turn = id; cancelling = false; errorMessage = undefined; sawActivity = false; pendingIdle = false;
      parts.clear(); partTypes.clear(); partOwners.clear(); roles.clear(); waitingParts.clear(); waitingBytes = 0;
    };
    const processEvent = event => {
      const p = event.properties || {};
      const sid = p.sessionID || p.part?.sessionID || p.info?.sessionID;
      if (sid !== session) return;
      if (!turn && previousTurn && !closing && event.type === 'session.status' && p.status?.type === 'busy') {
        startTurn(`wake:${++wakes}`);
        emit({ type: 'wake', session, after: previousTurn, turn });
      }
      if (!turn) return;
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
        if (part.type === 'step-finish' && part.tokens && !partTypes.has(part.id)) {
          const { input, output, cache, reasoning } = part.tokens;
          emit({ type: 'usage', turn, usage: { inputTokens: input, outputTokens: output,
            cachedReadTokens: cache?.read, cachedWriteTokens: cache?.write, thoughtTokens: reasoning } });
        }
        partTypes.set(part.id, part.type);
        partOwners.set(part.id, part.messageID);
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
          emit({ type: 'activity', turn, id: part.callID, kind: 'tool', title: state.title || part.tool, status,
            metadata: { ...state.metadata, tool: part.tool, rawResult: state.output, ...(typeof state.metadata?.exit === 'number' ? { exitCode: state.metadata.exit } : {}) },
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
        if (pendingPermissions.has(p.id)) return;
        pendingPermissions.add(p.id);
        void emitPermission(p);
      } else if (event.type === 'permission.replied') {
        pendingPermissions.delete(p.requestID);
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
        const cancelled = cancelling && p.error?.name === 'MessageAbortedError';
        previousTurn = undefined;
        emit({ type: 'done', turn, cancelled, error: cancelled ? undefined : errorMessage }); turn = null;
        pendingPermissions.clear(); questions.clear();
      }
      else if ((sawActivity || cancelling) && (event.type === 'session.idle' || (event.type === 'session.status' && p.status?.type === 'idle'))) {
        if (waitingParts.size) { pendingIdle = true; return; }
        const last = [...roles].findLast(([, role]) => role === 'assistant');
        let finalText;
        if (last) {
          finalText = '';
          for (const [id, owner] of partOwners) {
            if (owner !== last[0]) continue;
            if (partTypes.get(id) === 'tool') finalText = '';
            if (partTypes.get(id) === 'text') finalText += parts.get(id) ?? '';
          }
        }
        previousTurn = !cancelling && !errorMessage ? turn : undefined;
        emit({ type: 'done', turn, cancelled: cancelling, error: errorMessage, final_text: finalText }); turn = null; pendingPermissions.clear(); questions.clear();
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
      configOptions: [permissionOption('opencode_permission', '默认工具权限', rule, [
        ['default', '沿用本机配置'], ['ask', '询问（ask）'], ['allow', '允许（allow）'], ['deny', '拒绝（deny）'],
      ])],
      async prompt(text, id, images = [], documents = []) {
        if (turn) throw new Error('OpenCode already has an active turn');
        startTurn(id);
        await api(`/session/${session}/prompt_async`, 'POST', {
          ...(config.butlerInstructions ? { system: config.butlerInstructions } : {}),
          ...(model ? { model: splitModel(model) } : {}), ...(config.textOnly ? { tools: { '*': false } } : {}), parts: [{ type: 'text', text },
            ...images.map(({ mimeType, data }) => ({ type: 'file', mime: mimeType, url: `data:${mimeType};base64,${data}` })),
            ...documents.map(path => ({ type: 'file', mime: 'application/pdf', filename: basename(path), url: pathToFileURL(path).href }))],
        });
      },
      async cancel() {
        previousTurn = undefined;
        if (!turn) return;
        cancelling = true;
        try { await api(`/session/${session}/abort`, 'POST'); }
        catch (error) { cancelling = false; throw error; }
      },
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
        if (turn) throw new RequestRejected('Cannot change OpenCode configuration during a turn');
        if (key !== 'model') throw new RequestRejected(`Unsupported OpenCode configuration: ${key}`);
        splitModel(value); model = value;
      },
      models,
      close,
    };
  } catch (error) { await close(); throw error; }
}
