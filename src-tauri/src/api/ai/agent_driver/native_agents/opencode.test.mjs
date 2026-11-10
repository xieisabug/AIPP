import test from 'node:test';
import assert from 'node:assert/strict';
import { openOpenCode } from './opencode.mjs';
import { tick, openCodeFixture } from './test-helpers.mjs';

test('should authenticate the owned OpenCode server and merge MCP without replacing native permissions', async () => {
  const previous = process.env.OPENCODE_CONFIG_CONTENT;
  process.env.OPENCODE_CONFIG_CONTENT = JSON.stringify({ permission: { bash: 'ask' }, mcp: { existing: { type: 'remote', url: 'https://example.invalid/mcp' } } });
  const fixture = openCodeFixture();
  const mcp = { command: 'aipp', args: ['--aipp-acp-mcp-bridge'], env: { AIPP_SCOPE: 'synthetic-token' } };
  try {
    const runtime = await openOpenCode({ cwd: process.cwd(), session: 's1', mcp }, () => {}, fixture);
    try {
      const environment = fixture.launches[0][2].env;
      const config = JSON.parse(environment.OPENCODE_CONFIG_CONTENT);
      assert.deepEqual(config.permission, { bash: 'ask' });
      assert.equal(config.mcp.existing.url, 'https://example.invalid/mcp');
      assert.deepEqual(config.mcp.aipp.command, ['aipp', '--aipp-acp-mcp-bridge']);
      assert.deepEqual(config.mcp.aipp.environment, mcp.env);
      assert.ok(environment.OPENCODE_SERVER_PASSWORD.length >= 32);
      assert.equal(fixture.requests[0].headers.Authorization, `Basic ${Buffer.from(`opencode:${environment.OPENCODE_SERVER_PASSWORD}`).toString('base64')}`);
      assert.equal(fixture.launches[0][1].join(' ').includes('synthetic-token'), false);
    } finally { await runtime.close(); }
  } finally {
    if (previous === undefined) delete process.env.OPENCODE_CONFIG_CONTENT;
    else process.env.OPENCODE_CONFIG_CONTENT = previous;
  }
});

test('should replay OpenCode parts once when assistant identity metadata arrives late', async () => {
  const fixture = openCodeFixture();
  const runtime = await openOpenCode({ cwd: process.cwd(), session: 's1' }, event => fixture.events.push(event), fixture);
  try {
    await runtime.prompt('task', 'turn');
    fixture.event('message.part.updated', { part: { id: 'p1', messageID: 'late', sessionID: 's1', type: 'text', text: 'hello' } });
    fixture.event('message.part.delta', { partID: 'p1', messageID: 'late', sessionID: 's1', field: 'text', delta: ' world' });
    fixture.event('message.updated', { info: { id: 'late', sessionID: 's1', role: 'assistant' } });
    fixture.event('message.part.updated', { part: { id: 'p1', messageID: 'late', sessionID: 's1', type: 'text', text: 'hello world' } });
    await tick();
    assert.equal(fixture.events.filter(event => event.type === 'text').map(event => event.text).join(''), 'hello world');
    assert.equal(fixture.events.filter(event => event.type === 'text').length, 2);
  } finally { await runtime.close(); }
});

test('should preserve OpenCode tool failures and interruptions even when native state is completed', async () => {
  const fixture = openCodeFixture();
  const runtime = await openOpenCode({ cwd: process.cwd(), session: 's1' }, event => fixture.events.push(event), fixture);
  try {
    await runtime.prompt('task', 'turn');
    fixture.event('message.updated', { info: { id: 'a', sessionID: 's1', role: 'assistant' } });
    for (const [callID, state] of [
      ['exit', { status: 'completed', metadata: { exit: 1 }, output: 'test failed' }],
      ['abort', { status: 'error', error: 'Tool execution aborted' }],
      ['failure', { status: 'error', error: 'permission denied' }],
    ]) fixture.event('message.part.updated', { part: { id: callID, callID, messageID: 'a', sessionID: 's1', type: 'tool', tool: 'bash', state } });
    await tick();
    assert.deepEqual(fixture.events.filter(event => event.type === 'activity').map(event => event.status), ['failed', 'cancelled', 'failed']);
  } finally { await runtime.close(); }
});

test('should fail OpenCode restore without creating a session when the native identity differs', async () => {
  const fixture = openCodeFixture({ respond: path => path === '/session/s1' ? Response.json({ id: 'other' }) : undefined });
  await assert.rejects(openOpenCode({ cwd: process.cwd(), session: 's1' }, () => {}, fixture), /different session/);
  assert.equal(fixture.requests.some(request => request.path === '/session' && request.method === 'POST'), false);
  assert.equal(fixture.child.killed, true);
});

test('should report unexpected OpenCode event-stream completion during an active turn', async () => {
  const fixture = openCodeFixture();
  const runtime = await openOpenCode({ cwd: process.cwd(), session: 's1' }, event => fixture.events.push(event), fixture);
  await runtime.prompt('task', 'turn'); fixture.end(); await tick();
  assert.match(fixture.events.find(event => event.type === 'fatal').error, /stream ended unexpectedly/);
  assert.equal(fixture.events.some(event => event.type === 'done'), false);
  await runtime.close();
});

test('should send OpenCode abort while prompt HTTP admission is pending', async () => {
  let admit;
  const admission = new Promise(resolve => { admit = resolve; });
  const fixture = openCodeFixture({ respond: path => path.endsWith('/prompt_async') ? admission : undefined });
  const runtime = await openOpenCode({ cwd: process.cwd(), session: 's1' }, event => fixture.events.push(event), fixture);
  const prompt = runtime.prompt('task', 'turn');
  await runtime.cancel();
  assert.equal(fixture.requests.at(-1).path, '/session/s1/abort');
  admit(Response.json(true)); await prompt; await runtime.close();
});

test('should keep OpenCode session ownership and avoid echoing user parts or completing on stale idle', async () => {
  const fixture = openCodeFixture();
  const runtime = await openOpenCode({ cwd: process.cwd(), session: 's1', model: 'provider/model' }, event => fixture.events.push(event), fixture);
  try {
    await runtime.prompt('user input', 'turn1');
    fixture.event('session.idle', { sessionID: 's1' });
    fixture.event('message.updated', { info: { id: 'user1', sessionID: 's1', role: 'user' } });
    fixture.event('message.part.updated', { part: { id: 'p0', messageID: 'user1', sessionID: 's1', type: 'text', text: 'user input' } });
    await tick(); assert.equal(fixture.events.length, 0);
    fixture.event('session.status', { sessionID: 's1', status: { type: 'busy' } });
    fixture.event('message.updated', { info: { id: 'answer1', sessionID: 's1', role: 'assistant' } });
    fixture.event('message.part.updated', { part: { id: 'r1', messageID: 'answer1', sessionID: 's1', type: 'reasoning', text: 'think' } });
    fixture.event('message.part.delta', { partID: 'r1', messageID: 'answer1', sessionID: 's1', field: 'text', delta: ' more' });
    fixture.event('message.part.updated', { part: { id: 'p1', messageID: 'answer1', sessionID: 's1', type: 'text', text: 'A' } });
    fixture.event('message.part.delta', { partID: 'p1', messageID: 'answer1', sessionID: 's1', field: 'text', delta: 'B' });
    fixture.event('message.part.updated', { part: { id: 'p1', messageID: 'answer1', sessionID: 's1', type: 'text', text: 'AB' } });
    fixture.event('session.idle', { sessionID: 'other' });
    await tick(); assert.equal(fixture.events.some(e => e.type === 'done'), false);
    fixture.event('session.idle', { sessionID: 's1' }); await tick();
    assert.equal(fixture.events.filter(e => e.type === 'text').map(e => e.text).join(''), 'AB');
    assert.equal(fixture.events.filter(e => e.kind === 'reasoning').length, 2);
    assert.equal(fixture.events.filter(e => e.type === 'done').length, 1);
    assert.equal(fixture.requests.some(r => r.path === '/session' && r.method === 'POST'), false);
    assert.deepEqual(fixture.requests.find(r => r.path.endsWith('/prompt_async')).body.model, { providerID: 'provider', modelID: 'model' });
  } finally { await runtime.close(); }
});

test('should preserve advertised OpenCode permission decisions and reject stale replies', async () => {
  const fixture = openCodeFixture();
  const runtime = await openOpenCode({ cwd: process.cwd(), session: 's1' }, event => fixture.events.push(event), fixture);
  try {
    await runtime.prompt('task', 'turn');
    fixture.event('permission.asked', { id: 'approval', sessionID: 's1', permission: 'bash' }); await tick();
    await assert.rejects(runtime.reply('approval', 'unknown-option'), /Invalid OpenCode/);
    await runtime.reply('approval', 'reject');
    assert.deepEqual(fixture.requests.at(-1).body, { reply: 'reject' });
    await assert.rejects(runtime.reply('approval', 'once'), /expired/);
    fixture.event('question.asked', { id: 'question', sessionID: 's1', questions: [{ question: 'Pick items', multiple: true,
      options: [{ label: 'One, two', description: 'first' }, { label: 'Three', description: 'second' }] }] });
    await tick();
    await runtime.answer('question', { 'Pick items': 'One, two, Three' });
    assert.deepEqual(fixture.requests.at(-1).body, { answers: [['One, two', 'Three']] });
    await runtime.cancel();
    assert.equal(fixture.requests.at(-1).path, '/session/s1/abort');
  } finally { await runtime.close(); }
});

test('should reject unadvertised OpenCode custom answers without consuming the pending question', async () => {
  const fixture = openCodeFixture();
  const runtime = await openOpenCode({ cwd: process.cwd(), session: 's1' }, event => fixture.events.push(event), fixture);
  try {
    await runtime.prompt('task', 'turn');
    fixture.event('question.asked', { id: 'q', sessionID: 's1', questions: [{ question: 'Choice?', multiple: false, custom: false,
      options: [{ label: 'A', description: 'first' }, { label: 'B', description: 'second' }] }] });
    await tick();
    await assert.rejects(runtime.answer('q', { 'Choice?': 'C' }), /custom answer/);
    assert.equal(fixture.requests.some(request => request.path === '/question/q/reply'), false);
    await runtime.answer('q', { 'Choice?': 'B' });
    assert.deepEqual(fixture.requests.at(-1).body, { answers: [['B']] });
  } finally { await runtime.close(); }
});

test('should report OpenCode errors even before any assistant activity', async () => {
  const fixture = openCodeFixture();
  const runtime = await openOpenCode({ cwd: process.cwd(), session: 's1' }, event => fixture.events.push(event), fixture);
  try {
    await runtime.prompt('task', 'turn');
    fixture.event('session.error', { sessionID: 's1', error: { message: 'configured model is unavailable' } });
    await tick();
    assert.match(fixture.events.find(e => e.type === 'done').error, /configured model is unavailable/);
  } finally { await runtime.close(); }
});
