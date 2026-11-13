import test from 'node:test';
import assert from 'node:assert/strict';
import { loginCursor, loginUrl, openCursor, storedCredential } from './cursor.mjs';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { tick } from './test-helpers.mjs';

test('should pass the scoped MCP configuration to Cursor without enabling agent retries', async () => {
  let options;
  const mcp = { type: 'stdio', command: 'aipp', args: ['--aipp-acp-mcp-bridge'], env: { AIPP_SCOPE: 'synthetic-token' } };
  const sdk = { Agent: { async create(value) { options = value; return { agentId: 'id', async close() {} }; } } };
  const runtime = await openCursor({ cwd: process.cwd(), mcp }, () => {}, { sdk });
  assert.deepEqual(options.mcpServers, { aipp: mcp });
  assert.equal(options.local.enableAgentRetries, false);
  assert.equal(options.local.autoReview, true);
  await runtime.close();
});

function cursorFixture() {
  const turns = [], events = [];
  const agent = { agentId: 'cursor1', async send(_text, options) {
    let finish, fail;
    const completion = new Promise((resolve, reject) => { finish = resolve; fail = reject; });
    turns.push({ options, finish, fail });
    return { wait: () => completion, async cancel() {} };
  }, async close() {} };
  return { turns, events, sdk: { Agent: { async create() { return agent; } } } };
}

test('should split Cursor thinking phases at native completion and tool boundaries', async () => {
  const f = cursorFixture();
  const runtime = await openCursor({ cwd: '/repo' }, e => f.events.push(e), f);
  try {
    await runtime.prompt('task', 'turn');
    const delta = f.turns[0].options.onDelta;
    delta({ type: 'thinking-delta', text: 'before' });
    delta({ type: 'thinking-completed', thinkingDurationMs: 10 });
    delta({ type: 'tool-call-started', callId: 'shell', toolCall: { type: 'shell', args: { command: 'echo hi' } } });
    delta({ type: 'thinking-delta', text: 'after' });
    // A late update from an earlier tool must not end a newer thinking block.
    delta({ type: 'tool-call-completed', callId: 'shell', toolCall: { type: 'shell', result: { status: 'success', value: { stdout: 'hi', stderr: '', exitCode: 0 } } } });
    const call = f.events.at(-1);
    assert.equal(call.input.command, 'echo hi');
    assert.equal(call.output, 'hi');
    assert.equal(call.metadata.exitCode, 0);
    delta({ type: 'text-delta', text: 'answer' });
    const thoughts = f.events.filter(e => e.kind === 'reasoning');
    assert.deepEqual(thoughts.map(e => e.status), ['running', 'success', 'running', 'success']);
    assert.equal(thoughts[0].id, thoughts[1].id);
    assert.equal(thoughts[2].id, thoughts[3].id);
    assert.notEqual(thoughts[0].id, thoughts[2].id);
    f.turns[0].finish({ status: 'finished' }); await tick();
  } finally { await runtime.close(); }
});

test('should expose Cursor real edits and MCP identity and reject unsuccessful tool results', async () => {
  const f = cursorFixture();
  const runtime = await openCursor({ cwd: '/repo' }, e => f.events.push(e), f);
  try {
    await runtime.prompt('task', 'turn');
    const delta = f.turns[0].options.onDelta;
    delta({ type: 'tool-call-completed', callId: 'edit', toolCall: { type: 'edit', args: { path: 'a.rs' }, result: { status: 'success', value: { diffString: '+actual' } } } });
    assert.equal(f.events.at(-1).metadata.diff, '+actual');
    delta({ type: 'tool-call-completed', callId: 'shell', toolCall: { type: 'shell', args: { command: 'false' }, result: { status: 'success', value: { exitCode: 1, stdout: '', stderr: 'bad exit' } } } });
    assert.equal(f.events.at(-1).status, 'failed');
    assert.equal(f.events.at(-1).metadata.exitCode, 1);
    delta({ type: 'tool-call-completed', callId: 'mcp', toolCall: { type: 'mcp', args: { providerIdentifier: 'aipp', toolName: 'read', args: { path: 'remote' } }, result: { status: 'success', value: { isError: true, content: [{ text: { text: 'MCP rejected' } }] } } } });
    assert.equal(f.events.at(-1).metadata.server, 'aipp');
    assert.equal(f.events.at(-1).metadata.display_tool, 'read');
    assert.equal(f.events.at(-1).output, 'MCP rejected');
    assert.equal(f.events.at(-1).status, 'failed');
    f.turns[0].finish({ status: 'finished' }); await tick();
  } finally { await runtime.close(); }
});

for (const [output, failed] of [
  ['Error: RetriableError: WritableIterable is closed', true],
  ['Error: ConnectError: [unavailable] transport closed', true],
  ['Something went wrong communicating with the server. Please try again.', true],
  ['The error was Error: RetriableError: WritableIterable is closed', false],
  ['```text\nError: RetriableError: WritableIterable is closed\n```', false],
]) test(`should ${failed ? 'fail' : 'preserve'} Cursor final output when it ${failed ? 'is a transport diagnostic' : 'explains a diagnostic'}: ${output.slice(0,30)}`, async () => {
  const fixture = cursorFixture();
  const runtime = await openCursor({ cwd: process.cwd() }, event => fixture.events.push(event), fixture);
  await runtime.prompt('task', 'turn');
  fixture.turns[0].finish({ status: 'finished', result: output }); await tick();
  assert.equal(Boolean(fixture.events.at(-1).error), failed);
  if (!failed) assert.equal(fixture.events.find(event => event.type === 'text')?.text, output);
  assert.equal(fixture.turns.length, 1);
  await runtime.close();
});

test('should retain unexpected Cursor SDK errors when cancellation was requested', async () => {
  const fixture = cursorFixture();
  const runtime = await openCursor({ cwd: process.cwd() }, event => fixture.events.push(event), fixture);
  await runtime.prompt('task', 'turn'); await runtime.cancel();
  fixture.turns[0].fail(new Error('authentication failed')); await tick();
  assert.match(fixture.events.at(-1).error, /authentication failed/);
  assert.equal(fixture.events.at(-1).cancelled, false);
  await runtime.close();
});

test('should preserve failed Cursor tools when the SDK emits tool-call-completed with an error result', async () => {
  const fixture = cursorFixture();
  const runtime = await openCursor({ cwd: process.cwd() }, event => fixture.events.push(event), fixture);
  await runtime.prompt('task', 'turn');
  fixture.turns[0].options.onDelta({ type: 'tool-call-completed', callId: 'read1', toolCall: { type: 'read', result: { status: 'error', error: 'file not found' } } });
  assert.equal(fixture.events.at(-1).status, 'failed');
  assert.match(fixture.events.at(-1).error, /file not found/);
  fixture.turns[0].finish({ status: 'finished' }); await tick(); await runtime.close();
});

test('should ignore old Cursor callbacks when a later turn is already active', async () => {
  const fixture = cursorFixture();
  const runtime = await openCursor({ cwd: process.cwd() }, event => fixture.events.push(event), fixture);
  await runtime.prompt('first', 'turn1');
  fixture.turns[0].finish({ status: 'finished' }); await tick();
  await runtime.prompt('second', 'turn2');
  fixture.turns[0].options.onDelta({ type: 'text-delta', text: 'late' });
  fixture.turns[1].options.onDelta({ type: 'text-delta', text: 'current' });
  assert.deepEqual(fixture.events.filter(event => event.type === 'text').map(event => event.text), ['current']);
  fixture.turns[1].finish({ status: 'finished' }); await tick(); await runtime.close();
});

test('should identify SDK-wrapped Cursor aborts as cancellation when stop was requested', async () => {
  const fixture = cursorFixture();
  const runtime = await openCursor({ cwd: process.cwd() }, event => fixture.events.push(event), fixture);
  await runtime.prompt('task', 'turn'); await runtime.cancel();
  fixture.turns[0].fail(new Error('SDK request aborted', { cause: { name: 'ConnectError', cause: { name: 'AbortError' } } }));
  await tick();
  assert.equal(fixture.events.at(-1).cancelled, true);
  assert.equal(fixture.events.at(-1).error, undefined);
  await runtime.close();
});

test('should preserve Cursor run errors without retrying or recreating the agent', async () => {
  const fixture = cursorFixture();
  const runtime = await openCursor({ cwd: process.cwd() }, event => fixture.events.push(event), fixture);
  await runtime.prompt('task', 'turn');
  fixture.turns[0].finish({ status: 'error', error: { code: 'usage_limit', message: 'account limit reached' } });
  await tick();
  assert.match(fixture.events.at(-1).error, /account limit reached/);
  assert.equal(fixture.turns.length, 1);
  await runtime.close();
});

test('should use exact Cursor resume and SDK model objects without creating another agent', async () => {
  const events = []; let createCount = 0, options, sendOptions, complete;
  const finished = new Promise(resolve => { complete = resolve; });
  const agent = { agentId: 'existing', async send(_text, value) {
    sendOptions = value; value.onDelta({ type: 'text-delta', text: 'hello' });
    return { wait: () => finished, async cancel() { complete({ status: 'cancelled' }); } };
  }, async close() {} };
  const sdk = { Agent: { async create() { createCount++; return agent; }, async resume(id, value) { assert.equal(id, 'existing'); options = value; return agent; } } };
  const runtime = await openCursor({ cwd: process.cwd(), session: 'existing', model: 'chosen-model' }, event => events.push(event), { sdk });
  await runtime.prompt('hello', 'turn'); await runtime.cancel(); await tick(); await runtime.close();
  assert.equal(createCount, 0);
  assert.deepEqual(options.model, { id: 'chosen-model' });
  assert.deepEqual(sendOptions.model, { id: 'chosen-model' });
  assert.equal(options.local.enableAgentRetries, false);
  assert.equal(events.find(e => e.type === 'done').cancelled, true);
});

test('should propagate Cursor restore failure without falling back to create', async () => {
  let creates = 0;
  const sdk = { Agent: { async create() { creates++; }, async resume() { throw new Error('expired credentials'); } } };
  await assert.rejects(openCursor({ cwd: process.cwd(), session: 'existing' }, () => {}, { sdk }), /expired credentials/);
  assert.equal(creates, 0);
});

test('should use Cursor native default and cancel when send admission is still pending', async () => {
  let admit, cancelled = false, options;
  const admission = new Promise(resolve => { admit = resolve; });
  let complete;
  const finished = new Promise(resolve => { complete = resolve; });
  const sdk = { Agent: { async create(value) {
    options = value;
    return { agentId: 'new', send: () => admission, async close() {} };
  } } };
  const runtime = await openCursor({ cwd: process.cwd() }, () => {}, { sdk });
  const prompt = runtime.prompt('hello', 'turn');
  const cancel = runtime.cancel();
  admit({ wait: () => finished, async cancel() { cancelled = true; complete({ status: 'cancelled' }); } });
  await Promise.all([prompt, cancel]);
  assert.deepEqual(options.model, { id: 'default' });
  assert.equal(cancelled, true);
  await runtime.close();
});

test('should prefer CURSOR_API_KEY, then the provider sign-in, and reject expired sign-ins', async () => {
  const read = async () => JSON.stringify({ version: 1, apiKey: 'login-key', apiKeyExpiresAtMs: 2000, backendUrl: 'https://api2.cursor.sh', createdAtMs: 1 });
  assert.equal(await storedCredential('/p/auth.json', 1000, read), 'login-key');
  await assert.rejects(storedCredential('/p/auth.json', 3000, read), /登录已过期/);
  assert.equal(await storedCredential('/p/auth.json', 1000, async () => { throw Object.assign(new Error('missing'), { code: 'ENOENT' }); }), undefined);
  await assert.rejects(storedCredential('/p/auth.json', 1000, async () => '{broken'), /not valid JSON/);
  assert.equal(await storedCredential(undefined), undefined);
});

test('should open Cursor with the provider sign-in when no API key is configured', async () => {
  const previous = process.env.CURSOR_API_KEY; delete process.env.CURSOR_API_KEY;
  const dir = await mkdtemp(join(tmpdir(), 'aipp-cursor-'));
  try {
    const authStore = join(dir, 'auth.json');
    await writeFile(authStore, JSON.stringify({ version: 1, apiKey: 'login-key', backendUrl: 'https://api2.cursor.sh', createdAtMs: 1 }));
    let options;
    const sdk = { Agent: { async create(value) { options = value; return { agentId: 'id', async close() {} }; } } };
    const runtime = await openCursor({ cwd: process.cwd(), auth_store: authStore }, () => {}, { sdk });
    assert.equal(options.apiKey, 'login-key');
    await runtime.close();
    process.env.CURSOR_API_KEY = 'env-key';
    await (await openCursor({ cwd: process.cwd(), auth_store: authStore }, () => {}, { sdk })).close();
    assert.equal(options.apiKey, 'env-key');
  } finally {
    if (previous === undefined) delete process.env.CURSOR_API_KEY; else process.env.CURSOR_API_KEY = previous;
    await rm(dir, { recursive: true, force: true });
  }
});

test('should report a missing sign-in instead of calling the model catalog anonymously', async () => {
  const previous = process.env.CURSOR_API_KEY; delete process.env.CURSOR_API_KEY;
  try {
    const probe = await openCursor({ cwd: process.cwd(), probe: true }, () => {});
    await assert.rejects(probe.models(), /Cursor 未登录/);
  } finally { if (previous !== undefined) process.env.CURSOR_API_KEY = previous; }
});

test('should persist the browser sign-in only after the SDK login completes and surface only Cursor URLs', async () => {
  const events = [], saved = [];
  class InMemoryCredentialStore { async save(value) { this.value = value; } async load() { return this.value; } }
  class FileCredentialStore { constructor(path) { this.path = path; } async save(value) { saved.push([this.path, value]); } }
  const sdk = { InMemoryCredentialStore, FileCredentialStore, Cursor: { auth: { async login(options) {
    assert.equal(options.openBrowser, false);
    assert.equal(options.apiKeyName, 'AIPP - Demo');
    options.onLoginUrl('https://cursor.com/loginDeepControl?challenge=abc');
    assert.equal(saved.length, 0, 'nothing is written while the browser step is pending');
    await options.store.save({ version: 1, apiKey: 'minted', backendUrl: 'https://api2.cursor.sh', createdAtMs: 1, apiKeyExpiresAtMs: 9 });
    return { apiKey: 'minted', email: 'u@example.com', apiKeyExpiresAtMs: 9 };
  } } } };
  const store = join(tmpdir(), 'aipp-cursor-login', 'auth.json');
  const result = await loginCursor({ auth_store: store, key_name: 'AIPP - Demo', cwd: process.cwd() }, event => events.push(event), { sdk });
  assert.deepEqual(result, { authenticated: true, email: 'u@example.com', expires_at_ms: 9 });
  assert.deepEqual(events, [{ type: 'auth_url', url: 'https://cursor.com/loginDeepControl?challenge=abc' }]);
  assert.equal(saved[0][0], store);
  assert.equal(saved[0][1].apiKey, 'minted');
  await assert.rejects(loginCursor({ auth_store: 'relative/auth.json' }, () => {}, { sdk }), /absolute/);
  assert.throws(() => loginUrl('https://evil.example/cursor.com'), /Unexpected Cursor sign-in URL host/);
  assert.throws(() => loginUrl('http://cursor.com/login'), /Unexpected/);
  assert.equal(loginUrl('https://www.cursor.com/login?x=1'), 'https://www.cursor.com/login?x=1');
});
