import { createRequire } from 'node:module';
import { readFile } from 'node:fs/promises';
import { isAbsolute, resolve } from 'node:path';
import { deadline } from './common.mjs';

const loadSdk = config => createRequire(resolve(config.sdk_directory || config.cwd, 'package.json'))('@cursor/sdk');

// Per-provider browser sign-in written by loginCursor() in the SDK's StoredSdkCredentials shape.
export async function storedCredential(path, now = Date.now(), read = readFile) {
  if (!path) return undefined;
  let text;
  try { text = await read(path, 'utf8'); } catch (error) {
    if (error.code === 'ENOENT') return undefined;
    throw new Error(`Cannot read the Cursor sign-in at ${path}: ${error.message}`);
  }
  let value;
  try { value = JSON.parse(text); } catch { throw new Error(`The Cursor sign-in at ${path} is not valid JSON; sign in to Cursor again`); }
  if (typeof value?.apiKey !== 'string' || !value.apiKey) throw new Error(`The Cursor sign-in at ${path} has no API key; sign in to Cursor again`);
  if (typeof value.apiKeyExpiresAtMs === 'number' && value.apiKeyExpiresAtMs <= now) {
    throw new Error('Cursor 登录已过期，请在提供商设置中重新登录 Cursor');
  }
  return value.apiKey;
}

// CURSOR_API_KEY (field or environment) wins over the provider's browser sign-in.
const apiKeyFor = async config => process.env.CURSOR_API_KEY || await storedCredential(config.auth_store);

// Only Cursor's own login pages may be opened from the settings window.
export function loginUrl(value) {
  if (typeof value !== 'string' || value.length > 16384 || /\s/.test(value)) throw new Error('Invalid Cursor sign-in URL');
  const url = new URL(value);
  if (url.protocol !== 'https:' || url.username || url.password
    || !(url.hostname === 'cursor.com' || url.hostname.endsWith('.cursor.com'))) throw new Error(`Unexpected Cursor sign-in URL host: ${url.host}`);
  return value;
}

// Browser sign-in through the official SDK, persisted only after it fully succeeds.
export async function loginCursor(config, emit, dependencies = {}) {
  if (!config.auth_store || !isAbsolute(config.auth_store)) throw new Error('Cursor sign-in requires an absolute per-provider credential path');
  const sdk = dependencies.sdk || loadSdk(config);
  const pending = new sdk.InMemoryCredentialStore();
  const result = await sdk.Cursor.auth.login({ openBrowser: false, store: pending, apiKeyName: config.key_name || 'AIPP',
    onLoginUrl: url => emit({ type: 'auth_url', url: loginUrl(url) }) });
  const credentials = await pending.load();
  if (!credentials) throw new Error('Cursor sign-in finished without returning credentials');
  await new sdk.FileCredentialStore(config.auth_store).save(credentials);
  return { authenticated: true, email: result.email ?? null, expires_at_ms: result.apiKeyExpiresAtMs ?? null };
}

function isAbort(error) {
  const seen = new Set();
  while (error && typeof error === 'object' && !seen.has(error)) {
    if (error.name === 'AbortError') return true;
    seen.add(error); error = error.cause;
  }
  return false;
}

function transportDiagnostic(text) {
  if (typeof text !== 'string' || text.length > 16384) return undefined;
  const value = text.trim();
  const [first, ...rest] = value.split(/\r?\n/);
  const known = first === 'Error: RetriableError: WritableIterable is closed'
    || /^Error: ConnectError: \[unavailable\] .+$/.test(first)
    || first === 'Something went wrong communicating with the server. Please try again.';
  return known && rest.every(line => !line.trim() || /^\s+at\s/.test(line)) ? value : undefined;
}

export async function openCursor(config, emit, dependencies = {}) {
  const apiKey = await apiKeyFor(config);
  const models = async () => {
    if (!apiKey) throw new Error('Cursor 未登录：在提供商设置中点击「登录 Cursor」，或填写 Cursor API Key');
    const response = await fetch(`${(process.env.CURSOR_BACKEND_URL || 'https://api.cursor.com').replace(/\/$/, '')}/v1/models`,
      { headers: { Authorization: `Bearer ${apiKey}` }, signal: AbortSignal.timeout(30000) });
    if (!response.ok) throw new Error(`Cursor models HTTP ${response.status}: ${await response.text()}`);
    const data = await response.json();
    return data.items.map(model => ({ id: model.id, name: model.displayName }));
  };
  if (config.probe) return { models, async close() {} };
  const { Agent } = dependencies.sdk || loadSdk(config);
  const options = {
    model: { id: config.model || 'default' },
    mode: 'agent',
    local: { cwd: config.cwd, autoReview: true,
      settingSources: ['project', 'user', 'team', 'mdm', 'plugins'], enableAgentRetries: false },
    ...(config.mcp ? { mcpServers: { aipp: config.mcp } } : {}),
    ...(apiKey ? { apiKey } : {}),
  };
  const agent = config.session ? await Agent.resume(config.session, options) : await Agent.create(options);
  if (!agent.agentId || (config.session && agent.agentId !== config.session)) {
    await agent.close(); throw new Error('Cursor returned an invalid or different agent ID');
  }
  let run, admission, active = null, model = config.model;
  return {
    session: agent.agentId,
    models,
    async prompt(text, turn) {
      if (active) throw new Error('Cursor already has an active turn');
      const current = { cancelled: false };
      active = current;
      let sawText = false, diagnosticText = '';
      try {
        admission = Promise.resolve(agent.send(text, { ...(model ? { model: { id: model } } : {}), onDelta(update) {
          if (active !== current) return;
          if (update.type === 'text-delta') {
            sawText = true;
            if (diagnosticText !== null) diagnosticText = diagnosticText.length + update.text.length <= 16384 ? diagnosticText + update.text : null;
            emit({ type: 'text', turn, text: update.text });
          }
          else if (update.type === 'thinking-delta') emit({ type: 'activity', turn, id: 'thinking', kind: 'reasoning', status: 'running', output: update.text, append: true });
          else if (['tool-call-started', 'partial-tool-call', 'tool-call-completed'].includes(update.type)) {
            const result = update.toolCall?.result;
            const failed = result?.status === 'error' || result?.status === 'failed';
            emit({ type: 'activity', turn, id: update.callId, kind: 'tool', title: update.toolCall?.type,
              status: update.type === 'tool-call-completed' ? (failed ? 'failed' : 'success') : 'running', input: update.toolCall,
              error: failed ? JSON.stringify(result.error || result) : undefined,
              output: update.type === 'tool-call-completed' ? JSON.stringify(update.toolCall) : undefined });
          }
        } }));
        run = await admission;
        void run.wait().then(result => {
          if (active !== current) return;
          active = null; run = undefined;
          const diagnostic = result.status === 'finished' ? transportDiagnostic(result.result ?? diagnosticText) : undefined;
          const unexpected = !['finished', 'error', 'cancelled'].includes(result.status);
          const error = diagnostic || (result.status === 'error' ? JSON.stringify(result) : unexpected ? `Cursor returned unsupported run status: ${JSON.stringify(result)}` : undefined);
          if (!error && !sawText && typeof result.result === 'string' && result.result) emit({ type: 'text', turn, text: result.result });
          emit({ type: 'done', turn, error,
            cancelled: result.status === 'cancelled' });
        }, error => {
          if (active !== current) return;
          active = null; run = undefined;
          const cancelled = current.cancelled && isAbort(error);
          emit({ type: 'done', turn, cancelled, error: cancelled ? undefined : error.message });
        });
      } catch (error) {
        if (active === current) active = null;
        if (current.cancelled && isAbort(error)) emit({ type: 'done', turn, cancelled: true });
        else throw error;
      }
    },
    async cancel() {
      if (active) active.cancelled = true;
      if (active && !run && admission) {
        try { await deadline(admission, 'Cursor run admission before cancellation'); }
        catch (error) { if (!isAbort(error)) throw error; }
      }
      if (run) await run.cancel();
    },
    async configure(key, value) {
      if (active) throw new Error('Cannot change Cursor configuration during a turn');
      if (key !== 'model') throw new Error(`Unsupported Cursor configuration: ${key}`);
      model = value;
    },
    async close() { try { if (run) await run.cancel(); } finally { active = null; await agent.close(); } },
  };
}
