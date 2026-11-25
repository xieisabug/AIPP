// Official ACP transports for Kiro V3 and Antigravity. Driver identities and
// persistence remain separate from the user-configured generic ACP provider.
import { PassThrough } from 'node:stream';
import { StringDecoder } from 'node:string_decoder';
import { mkdir, mkdtemp, writeFile, rm } from 'node:fs/promises';
import { basename, isAbsolute, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { RequestRejected, deadline, jsonLines, launch, permissionOption, stopChild, thinkingStream, readableResult } from './common.mjs';

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

// The CLI implements full auto-approval at process startup. Standalone ACP uses
// the session modes it advertises. Account operations never need this flag.
function cliAutoApproval(config) {
  return (config.runtime || 'cli') === 'cli' && config.antigravity_mode === 'yolo'
    && !config.probe && !config.login && !config.logout;
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
  const args = runtime === 'cli' ? ['acp'] : process.platform === 'linux' ? ['--uid='] : [];
  if (cliAutoApproval(config)) args.push('--dangerously-skip-permissions');
  return { command: config.command || (runtime === 'cli' ? 'agy' : 'antigravity-acp'), args, env,
    cleanup: () => fs.rm(temporary, { recursive: true, force: true }) };
}

const choices = option => (option?.options ?? []).flatMap(entry => 'value' in entry ? [entry] : entry.options ?? []);

// Antigravity permission modes the assistant may choose, with the native IDs each may be advertised as.
export const ANTIGRAVITY_MODES = {
  default: ['default'],
  auto_edit: ['auto_edit', 'autoEdit', 'accept-edits', 'accept_edits', 'acceptEdits'],
  auto: ['auto'],
  plan: ['plan'],
  yolo: ['yolo'],
};
function sessionModels(options = [], legacy) {
  const model = options.find(option => option.category === 'model' || option.id === 'model');
  if (model) return choices(model).map(choice => ({ id: choice.value, name: choice.name || choice.value }));
  return (legacy?.availableModels ?? []).map(choice => ({ id: choice.modelId, name: choice.name || choice.modelId }));
}

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
  let legacyModels;
  let advertisedModes = [], selectedMode = config.antigravity_mode || 'default';
  let autopilot = config.kiro_autopilot || 'off';
  let sequence = 0, session, turn = null, closing = false, ready = false, fatal, options = [], cancelTimer;
  let started = false, cancelling = false, cancelSent = false;
  const pending = new Map(), approvals = new Map(), optionWaiters = new Set();
  const thinking = thinkingStream(emit);
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
  const modeOption = () => options.find(option => option.category === 'mode' || option.id === 'mode');
  const observeMode = id => {
    const match = Object.entries(ANTIGRAVITY_MODES).find(([, aliases]) => aliases.includes(id));
    if (match) selectedMode = cliAutoApproval(config) ? 'yolo' : match[0];
  };
  const availableModes = () => new Set([...advertisedModes, ...choices(modeOption() ?? {}).map(c => c.value)]);
  const sessionOptions = () => {
    const current = snapshotOptions(options);
    if (driver === 'kiro') {
      return [...current.filter(o => o.id !== 'autopilot'), permissionOption('autopilot', 'Kiro Autopilot', autopilot,
        [['off', '逐项审批（off）'], ['on', '自动执行（on）']])];
    }
    const supported = availableModes();
    const labels = { default: '默认', auto_edit: '自动接受编辑', auto: '自动审核', plan: '规划', yolo: '跳过审核' };
    const modes = Object.entries(ANTIGRAVITY_MODES).filter(([key, aliases]) =>
      aliases.some(id => supported.has(id)) || (key === 'yolo' && (config.runtime || 'cli') === 'cli' && supported.has('default')));
    return [...current.filter(o => o.id !== modeOption()?.id), permissionOption('antigravity_mode', 'Antigravity 权限模式',
      selectedMode, modes.map(([key]) => [key, `${labels[key]}（${key}）`]), 'mode')];
  };
  const updateOptions = next => {
    if (!Array.isArray(next)) return;
    options = next;
    if (ready && driver === 'kiro') {
      const reported = next.find(o => o.id === 'autopilot')?.currentValue;
      if (['off', 'on'].includes(reported)) autopilot = reported;
    }
    if (ready && driver === 'antigravity') observeMode(modeOption()?.currentValue);
    if (next.some(option => option.id === 'model' || option.category === 'model')) legacyModels = undefined;
    if (legacyModels) {
      options = [...next, { id: 'model', name: '模型', category: 'model', type: 'select',
        currentValue: legacyModels.currentModelId,
        options: sessionModels([], legacyModels).map(({ id, name }) => ({ value: id, name })) }];
    }
    for (const notify of optionWaiters) notify();
    if (ready) emit({ type: 'config_options', options: sessionOptions() });
  };
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
        if (driver === 'antigravity' && ready && update.sessionUpdate === 'current_mode_update') {
          observeMode(update.currentModeId);
          emit({ type: 'config_options', options: sessionOptions() });
        }
        if (turn === null) return;
        if (['agent_message_chunk', 'agent_thought_chunk', 'tool_call', 'tool_call_update'].includes(update.sessionUpdate)
          || (update.sessionUpdate === 'info' && update._meta?.kiro?.kind === 'turn_start')) {
          started = true; if (cancelling) sendCancel();
        }
        if (update.sessionUpdate === 'agent_message_chunk' && update.content?.type === 'text') { thinking.end(turn); emit({ type: 'text', turn, text: update.content.text }); }
        if (update.sessionUpdate === 'agent_thought_chunk' && update.content?.type === 'text') thinking.delta(turn, update.content.text);
        if (['tool_call', 'tool_call_update'].includes(update.sessionUpdate)) {
          if (update.sessionUpdate === 'tool_call') thinking.end(turn);
          if (!update.toolCallId) throw new Error(`${driver} tool update omitted toolCallId`);
          const event = { type: 'activity', turn, id: update.toolCallId, kind: 'tool',
            metadata: { ...(update.kind ? { native_kind: update.kind } : {}),
              ...(update.rawOutput !== undefined || update.content ? { rawResult: update.rawOutput ?? update.content } : {}) },
            ...(update.status ? { status: ({ completed: 'success', failed: 'failed', pending: 'running', in_progress: 'running' })[update.status] || update.status } : {}),
            ...(update.title ? { title: update.title } : {}),
            ...(update.rawInput !== undefined ? { input: update.rawInput } : {}),
            ...(update.rawOutput !== undefined || update.content ? { output: readableResult(update.rawOutput ?? update.content) } : {}) };
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
          toolMeta: frame.params.toolCall?._meta,
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
    if (typeof frame.error?.message === 'string') waiter.reject(new RequestRejected(`${driver} ${waiter.method}: ${frame.error.message} (code=${frame.error.code}, data=${JSON.stringify(frame.error.data ?? null)})`));
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
    if (turn !== null) throw new RequestRejected(`Cannot change ${driver} configuration during a turn`);
    if (driver === 'kiro' && key === 'autopilot') {
      if (!['off', 'on'].includes(value)) throw new RequestRejected(`Unsupported Kiro Autopilot setting '${value}'; expected off or on`);
      const response = await request('session/set_config_option', { sessionId: session, configId: 'autopilot', value });
      const reported = response.configOptions?.find(o => o.id === 'autopilot')?.currentValue;
      if (reported != null && reported !== value) throw new RequestRejected(`Kiro returned autopilot=${reported} after selecting ${value}`);
      autopilot = value;
      updateOptions(response.configOptions);
      return;
    }
    if (driver === 'antigravity' && key === 'antigravity_mode') {
      const aliases = ANTIGRAVITY_MODES[value];
      if (!Object.hasOwn(ANTIGRAVITY_MODES, value)) throw new RequestRejected(`Unsupported Antigravity permission mode '${value}'`);
      const mode = modeOption();
      const modeId = aliases.find(id => advertisedModes.includes(id));
      const optionValue = mode && aliases.find(id => choices(mode).some(choice => choice.value === id));
      if (modeId) await request('session/set_mode', { sessionId: session, modeId });
      else if (optionValue) {
        const response = await request('session/set_config_option', { sessionId: session, configId: mode.id, value: optionValue });
        const reported = response.configOptions?.find(o => o.id === mode.id)?.currentValue;
        if (reported != null && !aliases.includes(reported)) throw new RequestRejected(`Antigravity returned ${reported} after selecting ${value}`);
        updateOptions(response.configOptions);
      }
      else throw new RequestRejected(`Antigravity did not advertise the ${value} permission mode (advertised: ${[...availableModes()].join(', ') || 'none'})`);
      selectedMode = cliAutoApproval(config) ? 'yolo' : value;
      if (ready) emit({ type: 'config_options', options: sessionOptions() });
      return;
    }
    if (driver === 'kiro' && key !== 'model') {
      // Still waiting for the advertised choices. No configuration write has been sent.
      try { await waitOption(key); }
      catch (error) { throw new RequestRejected(error.message); }
    }
    const option = options.find(option => option.id === key);
    // Kiro's model option can arrive after session/new. Explicit values are
    // sent to Kiro itself; rejected values fail, never select another model.
    if (!(driver === 'kiro' && key === 'model') && (!option || !choices(option).some(choice => choice.value === value))) throw new RequestRejected(`${driver} unavailable configuration ${key}=${value}`);
    if (key === 'model' && legacyModels) {
      const selectedCatalog = legacyModels;
      await request('session/set_model', { sessionId: session, modelId: value });
      // A modern model option may have arrived while the legacy request was in flight.
      if (legacyModels === selectedCatalog) {
        selectedCatalog.currentModelId = value;
        updateOptions(options.filter(option => option.id !== 'model'));
      }
      return;
    }
    const result = await request('session/set_config_option', { sessionId: session, configId: key, value });
    updateOptions(result.configOptions);
  };
  try {
    const init = await request('initialize', { protocolVersion: 1, clientInfo: { name: 'aipp', title: 'AIPP', version: '0.4.3' },
      clientCapabilities: { fs: { readTextFile: false, writeTextFile: false }, terminal: false } });
    if (init.protocolVersion !== 1) throw new Error(`${driver} incompatible ACP protocol version: ${init.protocolVersion}`);
    if (config.logout && driver === 'antigravity') {
      if (!init.agentCapabilities?.auth?.logout) throw new Error('当前 Antigravity 版本不支持原生退出登录');
      await request('logout', {});
      return { close };
    }
    if (driver === 'antigravity') {
      const method = config.auth_method || 'oauth-personal';
      if (!init.authMethods?.some(item => item.id === method)) throw new Error(`Antigravity does not advertise authentication method ${method}`);
      await request('authenticate', { methodId: method }, config.login ? 300000 : 30000);
    }
    if (config.login || (driver === 'antigravity' && config.probe)) {
      return { close, async models() {
        const result = await request('session/new', { cwd: config.cwd, mcpServers: [] });
        return sessionModels(result.configOptions, result.models);
      } };
    }
    const sessionAppend = driver === 'antigravity' && Boolean(config.butlerInstructions)
      && (init.agentInfo?.version === '1.4.5' || init.agentCapabilities?._meta?.antigravity?.systemPrompt?.append === true);
    let instructionsPending = !sessionAppend && Boolean(config.butlerInstructions);
    const params = { cwd: config.cwd, mcpServers: config.mcp ? [{ name: 'aipp', command: config.mcp.command, args: config.mcp.args,
      env: Object.entries(config.mcp.env || {}).map(([name, value]) => ({ name, value: String(value) })) }] : [] };
    const canResume = driver === 'antigravity' || Boolean(init.agentCapabilities?.sessionCapabilities?.resume);
    const canLoad = driver === 'kiro' && Boolean(init.agentCapabilities?.loadSession);
    if (sessionAppend) params._meta = { antigravity: { systemPrompt: { append: config.butlerInstructions } } };
    let result;
    if (config.session) {
      const method = canResume ? 'session/resume' : 'session/load';
      if (method === 'session/load' && !canLoad) throw new Error(`${driver} does not support restoring this session`);
      session = config.session;
      result = await request(method, { ...params, sessionId: session });
      if (result.sessionId && result.sessionId !== session) throw new Error(`${driver} restored a different session`);
    } else {
      result = await request('session/new', params);
      session = result.sessionId;
      if (!session) throw new Error(`${driver} session/new omitted sessionId`);
    }
    if (driver === 'antigravity' && result.models?.availableModels?.length) legacyModels = result.models;
    updateOptions(result.configOptions ?? []);
    if (driver === 'kiro') {
      // V3 starts with Autopilot enabled. Write the chosen state (off unless the assistant
      // opted in) even before it advertises options, and fail if Kiro rejects it.
      await configure('autopilot', autopilot);
      if (config.kiro_agent) await configure('mode', config.kiro_agent);
    } else {
      // Reset any persisted execution mode (including plan) when the CLI
      // received the independent auto-approval flag on launch.
      advertisedModes = result.modes?.availableModes?.map(mode => mode.id) ?? [];
      await configure('antigravity_mode', cliAutoApproval(config) ? 'default' : selectedMode);
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
      promptCapabilities: { image: Boolean(init.agentCapabilities?.promptCapabilities?.image),
        audio: false, embedded_context: Boolean(init.agentCapabilities?.promptCapabilities?.embeddedContext) },
      session, canLoad, canResume, get configOptions() { return sessionOptions(); },
      async models() { return sessionModels(options); },
      configure,
      async prompt(text, id, images = [], documents = [], hasImagePaths = false) {
        if (turn !== null) throw new Error(`${driver} already has an active prompt`);
        turn = id; started = false; cancelling = false; cancelSent = false;
        try {
          const includeInstructions = instructionsPending && !/^\/[\w-]+(?:\s|$)/.test(text.trimStart());
          const capabilities = init.agentCapabilities?.promptCapabilities;
          if (images.length && !capabilities?.image && !hasImagePaths) {
            throw new Error(`${driver} 未声明 ACP 图片输入能力，无法发送这组图片`);
          }
          const prompt = [{ type: 'text', text }];
          if (capabilities?.image) prompt.push(...images.map(image => ({ type: 'image', ...image })));
          if (capabilities?.embeddedContext) prompt.push(...documents.map(path => ({
            type: 'resource_link', name: basename(path), uri: pathToFileURL(path).href, mimeType: 'application/pdf'
          })));
          if (includeInstructions) prompt.push({ type: 'text', text: config.butlerInstructions });
          const result = await request('session/prompt', { sessionId: session, prompt }, 24 * 60 * 60 * 1000);
          if (result.usage) emit({ type: 'usage', turn: id, usage: result.usage });
          if (!['end_turn', 'cancelled', 'max_tokens', 'max_turn_requests', 'refusal'].includes(result.stopReason)) throw new Error(`${driver} prompt omitted a supported stopReason: ${result.stopReason}`);
          if (result.stopReason === 'end_turn') {
            if (includeInstructions) instructionsPending = false;
            if (!sessionAppend && config.butlerInstructions && text.trim() === '/compact') instructionsPending = true;
          }
          thinking.end(id, result.stopReason === 'cancelled' ? 'cancelled' : result.stopReason === 'end_turn' ? 'success' : 'failed');
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
