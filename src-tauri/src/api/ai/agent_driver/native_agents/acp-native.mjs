// Official ACP transports for Kiro V3 and Antigravity. Driver identities and
// persistence remain separate from the user-configured generic ACP provider.
import { PassThrough } from 'node:stream';
import { StringDecoder } from 'node:string_decoder';
import { mkdir, mkdtemp, writeFile, rm } from 'node:fs/promises';
import { isAbsolute, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { deadline, jsonLines, launch, stopChild } from './common.mjs';

const AUTH_PREFIX = 'Open the following link to authenticate the ACP server: ';
const AUTH_MARKER = '__AIPP_ANTIGRAVITY_AUTH_URL__';
const AUTH_KEYS = new Set(['AIPP_ANTIGRAVITY_API_KEY', 'GEMINI_API_KEY', 'GOOGLE_API_KEY', 'GOOGLE_APPLICATION_CREDENTIALS',
  'GOOGLE_CLOUD_PROJECT', 'GOOGLE_CLOUD_LOCATION', 'GOOGLE_CLOUD_QUOTA_PROJECT',
  'GOOGLE_GENAI_USE_VERTEXAI', 'GCLOUD_PROJECT', 'CLOUDSDK_CORE_PROJECT', 'AGY_ACP_CCPA_PROJECT',
  'AGY_ACP_ENABLE_OAUTH', 'GEMINI_HOME', 'AGY_ACP_FORCE_FILE_STORAGE', 'ANTIGRAVITY_HARNESS_PATH',
  'BROWSER', 'PYTHONUNBUFFERED', 'CHROME_CRASHPAD_PIPE_NAME']);

export function authorizationUrl(value) {
  if (typeof value !== 'string' || value.length > 16384 || /\s/.test(value)) throw new Error('Invalid Antigravity authorization URL');
  const url = new URL(value);
  const redirect = url.searchParams.get('redirect_uri');
  const state = url.searchParams.get('state');
  if (url.origin !== 'https://accounts.google.com' || url.pathname !== '/o/oauth2/v2/auth'
    || url.username || url.password || url.hash || !state || state.length > 512
    || ['state', 'redirect_uri', 'response_type'].some(key => url.searchParams.getAll(key).length !== 1)
    || url.searchParams.get('response_type') !== 'code'
    || !/^http:\/\/127\.0\.0\.1:[1-9][0-9]{0,4}\/$/.test(redirect ?? '')
    || Number(new URL(redirect).port) < 1024) throw new Error('Invalid Antigravity authorization URL');
  return value;
}

export async function antigravityLaunch(config, environment = process.env, fs = { mkdir, mkdtemp, writeFile, rm }) {
  if (!config.profile || !isAbsolute(config.profile)) throw new Error('Antigravity requires an absolute per-provider account directory');
  const method = config.auth_method || 'oauth-personal';
  if (!['oauth-personal', 'gemini-api-key'].includes(method)) throw new Error(`Unsupported Antigravity authentication method: ${method}`);
  if (method === 'gemini-api-key' && !config.auth_key) throw new Error('Antigravity Gemini API key is missing');
  const runtime = config.runtime || 'cli';
  if (!['cli', 'standalone'].includes(runtime)) throw new Error(`Unsupported Antigravity runtime: ${runtime}`);
  if (runtime === 'standalone' && (!config.harness || !isAbsolute(config.harness))) throw new Error('Standalone Antigravity requires the matching localharness_external absolute path');
  const directory = join(config.profile, 'antigravity-acp');
  await fs.mkdir(directory, { recursive: true, mode: 0o700 });
  await fs.writeFile(join(directory, 'settings.json'), JSON.stringify({ auth: { type: method } }), { mode: 0o600 });
  const env = Object.fromEntries(Object.entries(environment).filter(([key]) => !AUTH_KEYS.has(key.toUpperCase())));
  const browser = [process.execPath, fileURLToPath(new URL('./auth-browser.mjs', import.meta.url))].map(path => {
    if (/["\r\n%]/.test(path) || path.includes(process.platform === 'win32' ? ';' : ':')) throw new Error('Antigravity browser helper path cannot be quoted safely');
    return `"${path.replaceAll('\\', '/')}"`;
  }).join(' ');
  const temporary = await fs.mkdtemp(join(config.temp_root || directory, 'antigravity-run-'));
  Object.assign(env, { GEMINI_HOME: config.profile, AGY_ACP_FORCE_FILE_STORAGE: '1',
    PYTHONUNBUFFERED: '1', BROWSER: `${browser} "%s"`, TEMP: temporary, TMP: temporary, TMPDIR: temporary });
  if (method === 'gemini-api-key') env.GEMINI_API_KEY = config.auth_key;
  if (runtime === 'standalone') env.ANTIGRAVITY_HARNESS_PATH = config.harness;
  return { command: config.command || (runtime === 'cli' ? 'agy' : 'antigravity-acp'),
    args: runtime === 'cli' ? ['acp'] : process.platform === 'linux' ? ['--uid='] : [], env,
    cleanup: () => fs.rm(temporary, { recursive: true, force: true }) };
}

const choices = option => (option?.options ?? []).flatMap(entry => 'value' in entry ? [entry] : entry.options ?? []);
const snapshotOptions = options => options.filter(option => option.type === 'select').map(option => ({
  id: option.id, name: option.name || option.id, category: option.category || option.id,
  current_value: option.currentValue, options: choices(option).map(choice => ({
    value: choice.value, name: choice.name || choice.value, description: choice.description ?? null, group_name: null,
  })),
}));

export async function openAcpNative(driver, config, emit, dependencies = {}) {
  if (!['kiro', 'antigravity'].includes(driver)) throw new Error(`Unknown native ACP driver: ${driver}`);
  const plan = driver === 'kiro'
    ? { command: config.command || 'kiro-cli', args: ['acp', '--agent-engine=v3', '--auth-method=cli'], env: process.env, cleanup: async () => {} }
    : await (dependencies.antigravityLaunch || antigravityLaunch)(config);
  let child;
  try { child = (dependencies.launch || launch)(plan.command, plan.args, { cwd: config.cwd, env: plan.env }); }
  catch (error) { await plan.cleanup(); throw error; }
  let sequence = 0, session, turn = null, closing = false, ready = false, fatal, options = [], cancelTimer;
  let started = false, cancelling = false, cancelSent = false;
  const pending = new Map(), approvals = new Map(), optionWaiters = new Set();
  const send = frame => child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', ...frame })}\n`);
  const fail = error => {
    if (fatal || closing) return;
    fatal = error;
    for (const waiter of pending.values()) waiter.reject(error);
    pending.clear();
    if (ready) emit({ type: 'fatal', error: error.message });
  };
  const request = async (method, params, milliseconds = 30000) => {
    if (fatal) throw fatal;
    const id = String(++sequence);
    try {
      return await deadline(new Promise((resolve, reject) => { pending.set(id, { resolve, reject, method }); send({ id, method, params }); }), `${driver} ${method}`, milliseconds);
    } finally { pending.delete(id); }
  };
  const updateOptions = next => { if (Array.isArray(next)) { options = next; for (const notify of optionWaiters) notify(); if (ready) emit({ type: 'config_options', options: snapshotOptions(options) }); } };
  const waitOption = async key => {
    const find = () => options.find(option => option.id === key || option.category === key);
    if (find()) return find();
    let notify;
    try {
      await deadline(new Promise(resolve => { notify = () => { if (find()) resolve(); }; optionWaiters.add(notify); }), `${driver} waiting for ${key} options`, 2000);
    } finally { optionWaiters.delete(notify); }
    return find();
  };
  const sendCancel = () => {
    if (turn === null || cancelSent) return;
    cancelSent = true; clearTimeout(cancelTimer);
    send({ method: 'session/cancel', params: { sessionId: session } });
  };
  const cancelApprovals = () => {
    for (const { id } of approvals.values()) send({ id, result: { outcome: { outcome: 'cancelled' } } });
    approvals.clear();
  };
  const onAuth = raw => {
    const url = authorizationUrl(raw);
    if (!config.login) throw new Error('Antigravity 尚未登录或登录已过期，请在提供商设置中点击“登录 Antigravity”');
    emit({ type: 'auth_url', url });
  };
  // Antigravity emits a non-JSON sign-in line on stdout and a browser helper
  // marker on stderr. Neither authorization URLs nor tokens enter diagnostics.
  const filter = (stream, protocol) => {
    const decoder = new StringDecoder('utf8'); let buffer = '';
    const line = value => {
      try {
        if (driver === 'antigravity' && value.startsWith(AUTH_PREFIX)) onAuth(value.slice(AUTH_PREFIX.length));
        else if (driver === 'antigravity' && value.startsWith(AUTH_MARKER)) onAuth(JSON.parse(value.slice(AUTH_MARKER.length)));
        else if (protocol) protocol.write(`${value}\n`);
        else process.stderr.write(`${value}\n`);
      } catch (error) { fail(error); }
    };
    stream.on('data', data => {
      buffer += decoder.write(data);
      let end;
      while ((end = buffer.indexOf('\n')) >= 0) { line(buffer.slice(0, end).replace(/\r$/, '')); buffer = buffer.slice(end + 1); }
      if (buffer.length > 16 * 1024 * 1024) { buffer = ''; fail(new Error(`${driver} protocol line exceeds 16 MiB`)); }
    });
    stream.on('end', () => { buffer += decoder.end(); if (buffer) line(buffer); protocol?.end(); });
    stream.on('error', fail);
  };
  child.on('error', fail);
  child.stdin.on('error', fail);
  child.on('exit', (code, signal) => fail(new Error(`${driver} process exited (code=${code}, signal=${signal})`)));
  const protocol = new PassThrough();
  jsonLines(protocol, frame => {
    if (frame.jsonrpc !== '2.0') throw new Error(`${driver} invalid JSON-RPC version`);
    if (frame.method) {
      if (frame.method === 'session/update') {
        if (session && frame.params?.sessionId !== session) return;
        const update = frame.params?.update;
        if (!update) throw new Error(`${driver} session/update omitted update`);
        if (update.sessionUpdate === 'config_option_update') updateOptions(update.configOptions);
        if (turn === null) return;
        if (['agent_message_chunk', 'agent_thought_chunk', 'tool_call', 'tool_call_update'].includes(update.sessionUpdate)
          || (update.sessionUpdate === 'info' && update._meta?.kiro?.kind === 'turn_start')) {
          started = true; if (cancelling) sendCancel();
        }
        if (update.sessionUpdate === 'agent_message_chunk' && update.content?.type === 'text') emit({ type: 'text', turn, text: update.content.text });
        if (update.sessionUpdate === 'agent_thought_chunk' && update.content?.type === 'text') emit({ type: 'activity', turn, id: 'thinking', kind: 'reasoning', status: 'running', append: true, output: update.content.text });
        if (['tool_call', 'tool_call_update'].includes(update.sessionUpdate)) {
          if (!update.toolCallId) throw new Error(`${driver} tool update omitted toolCallId`);
          const event = { type: 'activity', turn, id: update.toolCallId, kind: 'tool',
            ...(update.status ? { status: ({ completed: 'success', failed: 'failed', pending: 'running', in_progress: 'running' })[update.status] || update.status } : {}),
            ...(update.title ? { title: update.title } : {}),
            ...(update.rawInput !== undefined ? { input: update.rawInput } : {}),
            ...(update.rawOutput !== undefined || update.content ? { output: JSON.stringify(update.rawOutput ?? update.content) } : {}) };
          emit(event);
        }
      } else if (frame.method === 'session/request_permission') {
        if (turn === null || cancelling || frame.params?.sessionId !== session) { send({ id: frame.id, result: { outcome: { outcome: 'cancelled' } } }); return; }
        const rawOptions = frame.params.options;
        if (!Array.isArray(rawOptions) || !rawOptions.length) throw new Error(`${driver} permission request omitted options`);
        const optionIds = new Set();
        for (const option of rawOptions) {
          if (typeof option.optionId !== 'string' || !option.optionId.trim() || optionIds.has(option.optionId)
            || option.optionId === '__aipp_cancel_permission__') throw new Error(`${driver} permission request has invalid or duplicate option IDs`);
          optionIds.add(option.optionId);
        }
        const hasOnce = rawOptions.some(option => option.kind === 'allow_once');
        const offered = driver === 'kiro' && hasOnce ? rawOptions.filter(option => option.kind !== 'allow_always') : rawOptions;
        const id = String(frame.id);
        if (driver === 'antigravity' && frame.params.toolCall?.toolCallId?.startsWith('interaction_')) {
          if (offered.length < 2 || offered.length > 4) throw new Error(`Antigravity question has ${offered.length} choices; AIPP question cards support 2–4 choices`);
          const question = frame.params.toolCall.title || '请选择';
          const labels = offered.map(option => ({ label: `${option.name || option.optionId} [${option.optionId}]`, value: option.optionId }));
          approvals.set(id, { id: frame.id, options: offered, question, labels });
          emit({ type: 'question', turn, id, questions: [{ question, header: '选择', multiSelect: false,
            options: labels.map(option => ({ label: option.label, description: option.label })) }] });
          return;
        }
        approvals.set(id, { id: frame.id, options: offered });
        emit({ type: 'permission', turn, id, title: frame.params.toolCall?.title, input: frame.params.toolCall?.rawInput,
          options: [...offered.map(option => ({ id: option.optionId,
            name: `${option.name || option.optionId}${option.kind === 'allow_always' ? '（会保存到智能体权限规则）' : ''}${option._meta?.['agy.security.warning']?.message ? `：${String(option._meta['agy.security.warning'].message).slice(0, 512)}` : ''}`,
            kind: option.kind })),
            // ACP cancelled outcome is always available, even without reject_once.
            { id: '__aipp_cancel_permission__', name: '拒绝', kind: 'reject_once' }] });
      } else if (frame.id !== undefined) send({ id: frame.id, error: { code: -32601, message: `AIPP does not implement ${frame.method}` } });
      return;
    }
    const waiter = pending.get(String(frame.id));
    if (!waiter) return;
    pending.delete(String(frame.id));
    if (frame.error) waiter.reject(new Error(`${driver} ${waiter.method}: ${frame.error.message} (code=${frame.error.code}, data=${JSON.stringify(frame.error.data ?? null)})`));
    else if ('result' in frame) waiter.resolve(frame.result);
    else waiter.reject(new Error(`${driver} ${waiter.method} response omitted result/error`));
  }, fail);
  filter(child.stdout, protocol); filter(child.stderr);
  const close = async () => {
    closing = true; clearTimeout(cancelTimer); cancelApprovals();
    for (const waiter of pending.values()) waiter.reject(new Error(`${driver} session closed`));
    pending.clear(); await stopChild(child); await plan.cleanup();
  };
  const configure = async (key, value) => {
    if (turn !== null) throw new Error(`Cannot change ${driver} configuration during a turn`);
    if (driver === 'kiro' && key !== 'model') await waitOption(key);
    const option = options.find(option => option.id === key);
    // Kiro's model option can arrive after session/new. Explicit values are
    // sent to Kiro itself; rejected values fail, never select another model.
    if (!(driver === 'kiro' && key === 'model') && (!option || !choices(option).some(choice => choice.value === value))) throw new Error(`${driver} unavailable configuration ${key}=${value}`);
    const result = await request('session/set_config_option', { sessionId: session, configId: key, value });
    updateOptions(result.configOptions);
  };
  try {
    const init = await request('initialize', { protocolVersion: 1, clientInfo: { name: 'aipp', title: 'AIPP', version: '0.4.3' },
      clientCapabilities: { fs: { readTextFile: false, writeTextFile: false }, terminal: false } });
    if (init.protocolVersion !== 1) throw new Error(`${driver} incompatible ACP protocol version: ${init.protocolVersion}`);
    if (driver === 'antigravity') {
      const method = config.auth_method || 'oauth-personal';
      if (!init.authMethods?.some(item => item.id === method)) throw new Error(`Antigravity does not advertise authentication method ${method}`);
      await request('authenticate', { methodId: method }, config.login ? 300000 : 30000);
    }
    if (config.login) { ready = true; return { close }; }
    const params = { cwd: config.cwd, mcpServers: config.mcp ? [{ name: 'aipp', command: config.mcp.command, args: config.mcp.args,
      env: Object.entries(config.mcp.env || {}).map(([name, value]) => ({ name, value: String(value) })) }] : [] };
    let result;
    if (config.session) {
      const method = driver === 'antigravity' || init.agentCapabilities?.sessionCapabilities?.resume ? 'session/resume' : 'session/load';
      if (method === 'session/load' && !init.agentCapabilities?.loadSession) throw new Error(`${driver} does not support restoring this session`);
      session = config.session;
      result = await request(method, { ...params, sessionId: session });
      if (result.sessionId && result.sessionId !== session) throw new Error(`${driver} restored a different session`);
    } else {
      result = await request('session/new', params);
      session = result.sessionId;
      if (!session) throw new Error(`${driver} session/new omitted sessionId`);
    }
    updateOptions(result.configOptions);
    if (driver === 'kiro') {
      // V3 starts with Autopilot enabled. Write off even before it advertises
      // options, and fail if the native runtime rejects supervised operation.
      const response = await request('session/set_config_option', { sessionId: session, configId: 'autopilot', value: 'off' });
      updateOptions(response.configOptions);
      if (config.kiro_agent) await configure('mode', config.kiro_agent);
    } else {
      // Never inherit a persisted yolo/auto_edit permission mode.
      const mode = options.find(option => option.category === 'mode' || option.id === 'mode');
      if (result.modes?.availableModes?.some(mode => mode.id === 'default')) await request('session/set_mode', { sessionId: session, modeId: 'default' });
      else if (mode) await configure(mode.id, 'default');
      else throw new Error('Antigravity did not advertise supervised default mode');
    }
    if (config.model) await configure('model', config.model);
    if (config.kiro_effort) {
      // Kiro updates effort asynchronously after a model write. Do not silently
      // accept an unsupported level or reuse the previous model's choices.
      if (config.model) {
        let notify;
        const current = () => options.find(option => option.id === 'model')?.currentValue === config.model;
        try {
          if (!current()) await deadline(new Promise(resolve => { notify = () => { if (current()) resolve(); }; optionWaiters.add(notify); }), 'Kiro model advertisement before effort', 2000);
        } finally { optionWaiters.delete(notify); }
      }
      await configure('effortLevel', config.kiro_effort);
    }
    if (config.probe && driver === 'kiro') await waitOption('model');
    ready = true;
    return {
      session, get configOptions() { return snapshotOptions(options); },
      async models() { return choices(options.find(option => option.category === 'model' || option.id === 'model')).map(choice => ({ id: choice.value, name: choice.name || choice.value })); },
      configure,
      async prompt(text, id) {
        if (turn !== null) throw new Error(`${driver} already has an active prompt`);
        turn = id; started = false; cancelling = false; cancelSent = false;
        try {
          const result = await request('session/prompt', { sessionId: session, prompt: [{ type: 'text', text }] }, 24 * 60 * 60 * 1000);
          if (!['end_turn', 'cancelled', 'max_tokens', 'max_turn_requests', 'refusal'].includes(result.stopReason)) throw new Error(`${driver} prompt omitted a supported stopReason: ${result.stopReason}`);
          emit({ type: 'done', turn: id, cancelled: result.stopReason === 'cancelled',
            ...(['max_tokens', 'max_turn_requests', 'refusal'].includes(result.stopReason) ? { error: `${driver} stopped: ${result.stopReason}` } : {}) });
        } finally { clearTimeout(cancelTimer); cancelApprovals(); turn = null; }
      },
      async cancel() {
        cancelling = true; cancelApprovals();
        // Kiro 2.27 may drop cancellation before prompt starts (T3 regression).
        if (driver !== 'kiro' || started) sendCancel();
        else if (turn !== null) cancelTimer = setTimeout(sendCancel, 1000);
      },
      async reply(id, value) {
        const approval = approvals.get(id);
        if (!approval) throw new Error(`${driver} permission is expired or unknown`);
        if (value !== '__aipp_cancel_permission__' && !approval.options.some(option => option.optionId === value)) throw new Error(`${driver} permission option was not offered`);
        approvals.delete(id);
        send({ id: approval.id, result: { outcome: value === '__aipp_cancel_permission__' ? { outcome: 'cancelled' } : { outcome: 'selected', optionId: value } } });
      },
      async answer(id, answers) {
        const question = approvals.get(id);
        if (!question?.question) throw new Error(`${driver} question is expired or unknown`);
        const answer = answers?.[question.question];
        const selected = question.labels.find(option => option.label === answer);
        if (answers && !selected) throw new Error(`${driver} question answer must match an offered choice`);
        approvals.delete(id);
        send({ id: question.id, result: { outcome: selected ? { outcome: 'selected', optionId: selected.value } : { outcome: 'cancelled' } } });
      }, close,
    };
  } catch (error) { await close(); throw error; }
}
