import test from 'node:test';
import assert from 'node:assert/strict';
import { openCursor } from './cursor.mjs';
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
