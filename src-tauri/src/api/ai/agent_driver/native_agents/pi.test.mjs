import test from 'node:test';
import assert from 'node:assert/strict';
import { openPi } from './pi.mjs';
import { piFixture } from './test-helpers.mjs';

test('should inject the scoped MCP bridge into the Pi extension when tools are selected', async () => {
  const fixture = piFixture();
  const mcp = { command: 'aipp', args: ['--aipp-acp-mcp-bridge'], env: { AIPP_SCOPE: 'synthetic-token' } };
  const runtime = await openPi({ cwd: process.cwd(), mcp }, () => {}, fixture);
  try {
    assert.deepEqual(JSON.parse(fixture.launches[0][2].env.AIPP_PI_MCP), mcp);
    assert.equal(fixture.launches[0][1].includes('--extension'), true);
    assert.equal(fixture.launches[0][1].join(' ').includes('synthetic-token'), false);
  } finally { await runtime.close(); }
});

// T3 provider-pi adapter.test.ts: persistent identity, command admission and transport failure.
test('should reject a different persisted Pi identity when restoration appears successful', async () => {
  const fixture = piFixture();
  await assert.rejects(openPi({ cwd: process.cwd(), session: '/expected.jsonl' }, () => {}, fixture), /different sessionFile/);
  assert.equal(fixture.child.killed, true);
  assert.equal(fixture.commands.some(command => command.type === 'new_session'), false);
});

test('should finish Pi command-only prompts when native preflight marks them handled', async () => {
  const fixture = piFixture({ receipt: { disposition: 'handled' }, state: { sessionFile: '/native/session.jsonl', isStreaming: false } });
  const events = [];
  const runtime = await openPi({ cwd: process.cwd() }, event => events.push(event), fixture);
  try {
    await runtime.prompt('/extension-command', 'turn');
    assert.equal(events.filter(event => event.type === 'done').length, 1);
    assert.equal(fixture.commands.filter(command => command.type === 'prompt').length, 1);
  } finally { await runtime.close(); }
});

test('should report Pi extension execution errors when no model terminal event follows', async () => {
  const fixture = piFixture(); const events = [];
  const runtime = await openPi({ cwd: process.cwd() }, event => events.push(event), fixture);
  try {
    await runtime.prompt('task', 'turn');
    fixture.emit({ type: 'extension_error', extensionPath: '/extensions/custom.ts', event: 'tool_call', error: 'permission hook failed' });
    assert.match(events.find(event => event.type === 'fatal')?.error || '', /permission hook failed/);
  } finally { await runtime.close(); }
});

test('should retain Pi exit code and signal when the process dies during a turn', async () => {
  const fixture = piFixture(); const events = [];
  const runtime = await openPi({ cwd: process.cwd() }, event => events.push(event), fixture);
  await runtime.prompt('task', 'turn');
  fixture.child.emit('exit', 17, null);
  assert.match(events.find(event => event.type === 'fatal').error, /code=17/);
  await runtime.close();
});

test('should fail restoration without creating a replacement Pi session', async () => {
  const fixture = piFixture({ rejectRestore: true });
  await assert.rejects(openPi({ cwd: process.cwd(), session: '/missing.jsonl' }, () => {}, fixture), /session file missing/);
  assert.deepEqual(fixture.commands.map(c => c.type), ['switch_session']);
  assert.equal(fixture.child.killed, true);
});

test('should forward Pi text and approval while accepting abort before the user replies', async () => {
  const fixture = piFixture(); const events = [];
  const runtime = await openPi({ cwd: process.cwd() }, event => events.push(event), fixture);
  try {
    await runtime.prompt('hello', 'turn1');
    fixture.emit({ type: 'message_update', assistantMessageEvent: { type: 'text_delta', delta: 'answer' } });
    fixture.emit({ type: 'extension_ui_request', id: 'approval', method: 'confirm', title: 'Run command?', message: 'echo hi' });
    await runtime.cancel();
    assert.equal(fixture.commands.at(-1).type, 'abort');
    await runtime.reply('approval', 'deny');
    assert.equal(fixture.commands.at(-1).confirmed, false);
    fixture.emit({ type: 'extension_ui_request', id: 'question', method: 'input', title: 'Which file?' });
    await runtime.answer('question', { 'Which file?': 'README.md' });
    assert.equal(fixture.commands.at(-1).value, 'README.md');
    fixture.emit({ type: 'agent_end' });
    assert.equal(events.find(e => e.type === 'text').text, 'answer');
    assert.equal(events.find(e => e.type === 'permission').turn, 'turn1');
    assert.equal(events.filter(e => e.type === 'done').length, 1);
    await runtime.prompt('next', 'turn2');
    fixture.emit({ type: 'message_end', message: { role: 'assistant', stopReason: 'error', errorMessage: 'invalid model' } });
    fixture.emit({ type: 'agent_end' });
    assert.equal(events.at(-1).error, 'invalid model');
  } finally { await runtime.close(); }
  assert.equal(events.some(e => e.type === 'fatal'), false);
});

test('should pass the assistant approval scope to the Pi extension and reject unknown scopes', async () => {
  for (const [configured, expected] of [[undefined, 'all'], ['read_only', 'read_only'], ['none', 'none']]) {
    const fixture = piFixture();
    const runtime = await openPi({ cwd: process.cwd(), pi_approval: configured }, () => {}, fixture);
    try { assert.equal(fixture.launches[0][2].env.AIPP_PI_APPROVAL, expected); } finally { await runtime.close(); }
  }
  const fixture = piFixture();
  await assert.rejects(openPi({ cwd: process.cwd(), pi_approval: 'yolo' }, () => {}, fixture), /Unsupported Pi approval scope 'yolo'/);
  assert.equal(fixture.launches.length, 0, 'an invalid scope never starts Pi');
});

test('should classify Pi operations and preserve input and readable results across execution updates', async () => {
  const fixture = piFixture(), events = [];
  const runtime = await openPi({ cwd: '/repo' }, event => events.push(event), fixture);
  try {
    await runtime.prompt('task', 'turn');
    const operations = [
      ['bash', { command: 'echo hi' }, 'command', 'echo hi'],
      ['powershell', { command: 'Get-ChildItem' }, 'command', 'Get-ChildItem'],
      ['read', { path: 'README.md', offset: 2, limit: 10 }, 'file_read', '读取 README.md'],
      ['grep', { pattern: 'needle', path: 'src' }, 'file_search', '搜索 needle src'],
      ['find', { pattern: '*.rs' }, 'file_search', '搜索 *.rs'],
      ['ls', { path: 'src' }, 'file_search', '列出目录 src'],
      ['edit', { path: 'a.ts', edits: [{ oldText: 'a', newText: 'b' }] }, 'patch', '编辑 a.ts'],
      ['write', { path: 'existing.ts', content: 'b' }, 'file_write', '写入 existing.ts'],
      ['mcp_read_file', { path: 'remote' }, 'tool', 'mcp_read_file'],
    ];
    for (const [toolName, args, kind, title] of operations) {
      const toolCallId = `call:${toolName}`;
      fixture.emit({ type: 'tool_execution_start', toolCallId, toolName, args });
      fixture.emit({ type: 'tool_execution_update', toolCallId, toolName, args, partialResult: { content: [{ type: 'text', text: 'partial' }] } });
      const result = { content: [{ type: 'text', text: 'done' }], details: { diff: '+b', patch: '@@\n-a\n+b' } };
      fixture.emit({ type: 'tool_execution_end', toolCallId, toolName, result, isError: false });
      const updates = events.filter(event => event.id === toolCallId);
      assert.equal(updates.length, 3);
      assert.deepEqual(updates.map(event => event.kind), [kind, kind, kind]);
      assert.equal(updates[0].output, undefined, 'no fake null output before a result exists');
      assert.equal(updates[1].output, 'partial');
      assert.equal(updates[2].title, title);
      assert.deepEqual(updates[2].input, args, 'end events omit args; retain the original invocation');
      assert.equal(updates[2].output, 'done');
      assert.equal(updates[2].status, 'success');
      assert.deepEqual(updates[2].metadata.rawResult, result);
      assert.equal(updates[2].metadata.cwd, '/repo');
      if (toolName === 'edit') assert.equal(updates[2].metadata.changes[0].diff, result.details.patch);
      if (toolName === 'write') assert.equal(updates[2].metadata.changes, undefined, 'write does not imply file creation');
    }
    fixture.emit({ type: 'agent_end' });
    await runtime.prompt('next', 'next');
    fixture.emit({ type: 'tool_execution_start', toolCallId: 'call:read', toolName: 'read', args: { path: 'other.md' } });
    assert.equal(events.at(-1).title, '读取 other.md');
    assert.equal(events.at(-1).metadata.rawResult, undefined, 'new turns do not inherit old results');
  } finally { await runtime.close(); }
});

test('should preserve Pi tool failures and finish individual thinking blocks when native events arrive', async () => {
  const fixture = piFixture(), events = [];
  const runtime = await openPi({ cwd: '/repo' }, event => events.push(event), fixture);
  try {
    await runtime.prompt('task', 'turn');
    fixture.emit({ type: 'message_update', assistantMessageEvent: { type: 'thinking_delta', delta: 'first' } });
    fixture.emit({ type: 'message_update', assistantMessageEvent: { type: 'thinking_end' } });
    const thought = events.at(-1);
    assert.equal(thought.kind, 'reasoning');
    assert.equal(thought.status, 'success');
    assert.equal(thought.id, events.at(-2).id);
    fixture.emit({ type: 'message_update', assistantMessageEvent: { type: 'thinking_delta', delta: 'next' } });
    assert.notEqual(events.at(-1).id, thought.id);
    fixture.emit({ type: 'tool_execution_start', toolCallId: 'read', toolName: 'read', args: { path: 'missing' } });
    fixture.emit({ type: 'tool_execution_end', toolCallId: 'read', toolName: 'read', result: { content: [{ type: 'text', text: 'ENOENT: missing' }] }, isError: true });
    assert.equal(events.at(-1).status, 'failed');
    assert.equal(events.at(-1).error, 'ENOENT: missing');
    assert.equal(events.at(-1).kind, 'file_read');
    await runtime.cancel();
    assert.equal(fixture.commands.at(-1).type, 'abort');
  } finally { await runtime.close(); }
});
