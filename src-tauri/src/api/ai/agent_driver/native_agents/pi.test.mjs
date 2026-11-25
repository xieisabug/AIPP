import test from 'node:test';
import assert from 'node:assert/strict';
import { copyFile, mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { fork } from 'node:child_process';
import { createInterface } from 'node:readline';
import { pathToFileURL } from 'node:url';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { openPi } from './pi.mjs';
import { piFixture } from './test-helpers.mjs';

test('should skip Pi confirmation only for a registered bridge tool cleared with its actual arguments', async () => {
  const root = await mkdtemp(join(tmpdir(), 'aipp-pi-permission-'));
  const previous = process.env.AIPP_PI_MCP;
  const previousApproval = process.env.AIPP_PI_APPROVAL;
  const hooks = new Map(), tools = [];
  try {
    for (const file of ['common.mjs', 'pi-extension.mjs']) {
      await copyFile(new URL(file, import.meta.url), join(root, file));
    }
    const dependency = join(root, 'node_modules', 'typebox');
    await mkdir(dependency, { recursive: true });
    await writeFile(join(dependency, 'package.json'), JSON.stringify({ type: 'module', exports: './index.js' }));
    await writeFile(join(dependency, 'index.js'), 'export const Type = { Unsafe: schema => schema };');
    const fixture = join(root, 'bridge.mjs');
    await writeFile(fixture, `
      import { createInterface } from 'node:readline';
      for await (const line of createInterface({input: process.stdin})) {
        const {id,method,params} = JSON.parse(line);
        if (!id) continue;
        let result = {};
        if (method === 'tools/list') result = {tools:[{name:'aipp_t42_schedule_task'}]};
        if (method === 'aipp/reviewExemption') result = {exempt: params.name === 'aipp_t42_schedule_task' && params.arguments.action === 'list'};
        console.log(JSON.stringify({id,result}));
      }
    `);
    process.env.AIPP_PI_MCP = JSON.stringify({command:process.execPath,args:[fixture]});
    process.env.AIPP_PI_APPROVAL = 'all';
    const {default: install} = await import(pathToFileURL(join(root, 'pi-extension.mjs')).href);
    await install({on: (name, fn) => hooks.set(name, fn), registerTool: t => tools.push(t), getAllTools: () => tools});
    const denied = {ui:{confirm: async () => false}};
    const run = (toolName, action) => hooks.get('tool_call')({toolName,input:{action}}, denied);
    assert.equal(await run('aipp_t42_schedule_task','list'), undefined);
    assert.equal((await run('aipp_t42_schedule_task','delete')).block, true);
    assert.equal((await run('bash','list')).block, true);
    tools[0] = {...tools[0], parameters:{}, sourceInfo:{path:'other-extension.mjs'}};
    assert.equal((await run('aipp_t42_schedule_task','list')).block, true);
  } finally {
    await hooks.get('session_shutdown')?.();
    if (previous === undefined) delete process.env.AIPP_PI_MCP; else process.env.AIPP_PI_MCP = previous;
    if (previousApproval === undefined) delete process.env.AIPP_PI_APPROVAL; else process.env.AIPP_PI_APPROVAL = previousApproval;
    await rm(root, {recursive:true,force:true});
  }
});

test('should report every completed model request once when Pi repeats messages at agent_end', async () => {
  const f = piFixture(), events = [];
  const runtime = await openPi({ cwd: process.cwd() }, event => events.push(event), f);
  try {
    await runtime.prompt('task', 'turn');
    const messages = ['toolUse', 'stop'].map(stopReason => ({ role: 'assistant', stopReason,
      usage: { input: 10, output: 6, cacheRead: 30, cacheWrite: 4 } }));
    for (const message of messages) f.emit({ type: 'message_end', message });
    f.emit({ type: 'agent_end', messages });
    const reports = events.filter(e => e.type === 'usage');
    assert.equal(reports.length, 2);
    assert.deepEqual(reports[0], { type: 'usage', turn: 'turn', usage: {
      inputTokens: 10, outputTokens: 6, cachedReadTokens: 30, cachedWriteTokens: 4 } });
  } finally { await runtime.close(); }
});

test('should supply long Butler instructions to Pi by file on actor startup', async () => {
  const root = await mkdtemp(join(tmpdir(), 'pi-context-test-'));
  const role = '管家上下文'.repeat(10000);
  const fixture = piFixture();
  let runtime;
  try {
    runtime = await openPi({ cwd: root, temp_root: root, butlerInstructions: role }, () => {}, fixture);
    const [, args, options] = fixture.launches[0];
    assert.equal(await readFile(options.env.AIPP_PI_CONTEXT_FILE, 'utf8'), role);
    assert.equal(Object.values(options.env).includes(role), false);
    assert.equal(args.includes(role), false);
  } finally {
    await runtime?.close();
    await rm(root, { recursive: true, force: true });
  }
});

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
    fixture.emit({ type: 'agent_end', messages: [
      {role:'assistant', stopReason:'toolUse', content:[{type:'text', text:'先检查'}]},
      {role:'assistant', stopReason:'stop', content:[{type:'thinking', thinking:'过程'}, {type:'text', text:'answer'}]},
    ] });
    assert.equal(events.find(e => e.type === 'text').text, 'answer');
    assert.equal(events.at(-1).final_text, 'answer');
    assert.equal(events.find(e => e.type === 'permission').turn, 'turn1');
    assert.equal(events.filter(e => e.type === 'done').length, 1);
    await runtime.prompt('next', 'turn2');
    fixture.emit({ type: 'message_end', message: { role: 'assistant', stopReason: 'error', errorMessage: 'invalid model' } });
    fixture.emit({ type: 'agent_end' });
    assert.equal(events.at(-1).error, 'invalid model');
  } finally { await runtime.close(); }
  assert.equal(events.some(e => e.type === 'fatal'), false);
});

test('should retain the original turn through Pi retries and deliver just the recovered answer', async () => {
  const fixture = piFixture(), events = [];
  const runtime = await openPi({ cwd: process.cwd() }, event => events.push(event), fixture);
  try {
    await runtime.prompt('task', 'original');
    for (let attempt = 1; attempt <= 2; attempt++) {
      const failed = { role: 'assistant', stopReason: 'error', errorMessage: 'other side closed (UND_ERR_SOCKET)' };
      fixture.emit({ type: 'message_end', message: failed });
      fixture.emit({ type: 'agent_end', messages: [failed], willRetry: true });
      fixture.emit({ type: 'auto_retry_start', attempt });
      assert.equal(events.some(e => e.type === 'done'), false);
      await assert.rejects(runtime.prompt('next', 'other'), /active turn/);
    }
    const answer = { role: 'assistant', stopReason: 'stop', content: [{ type: 'text', text: '恢复后的答复' }] };
    fixture.emit({ type: 'message_end', message: answer });
    fixture.emit({ type: 'auto_retry_end', success: true, attempt: 2 });
    fixture.emit({ type: 'agent_end', messages: [answer], willRetry: false });
    fixture.emit({ type: 'agent_settled', aborted: false });
    const terminals = events.filter(e => e.type === 'done');
    assert.equal(terminals.length, 1);
    assert.equal(terminals[0].turn, 'original');
    assert.equal(terminals[0].final_text, '恢复后的答复');
    assert.equal(terminals[0].error, undefined);
    assert.equal(fixture.commands.filter(command => command.type === 'prompt').length, 1);
  } finally { await runtime.close(); }
});

test('should report the last Pi retry failure and release a turn cancelled during backoff', async () => {
  const fixture = piFixture(), events = [];
  const runtime = await openPi({ cwd: process.cwd() }, event => events.push(event), fixture);
  try {
    const first = { role: 'assistant', stopReason: 'error', errorMessage: 'initial connection failure' };
    await runtime.prompt('first task', 'failed-turn');
    fixture.emit({ type: 'agent_end', messages: [first], willRetry: true });
    const finalError = 'other side closed (UND_ERR_SOCKET)';
    fixture.emit({ type: 'agent_end', messages: [{ ...first, errorMessage: finalError }], willRetry: false });
    fixture.emit({ type: 'auto_retry_end', success: false, finalError });
    fixture.emit({ type: 'agent_settled', aborted: false });
    await runtime.prompt('second task', 'cancelled-turn');
    fixture.emit({ type: 'agent_end', messages: [first], willRetry: true });
    await runtime.cancel();
    fixture.emit({ type: 'auto_retry_end', success: false, finalError: 'Retry cancelled' });
    fixture.emit({ type: 'agent_settled', aborted: true });
    const terminals = events.filter(e => e.type === 'done');
    assert.deepEqual(terminals.map(e => [e.turn, e.error, e.cancelled]), [
      ['failed-turn', finalError, false], ['cancelled-turn', undefined, true],
    ]);
  } finally { await runtime.close(); }
});

test('should pass the assistant approval scope to the Pi extension and reject unknown scopes', async () => {
  for (const [configured, expected] of [[undefined, 'all'], ['read_only', 'read_only'], ['none', 'none']]) {
    const fixture = piFixture();
    const runtime = await openPi({ cwd: process.cwd(), session: '/native/session.jsonl', pi_approval: configured }, () => {}, fixture);
    try {
      assert.equal(fixture.launches[0][2].env.AIPP_PI_APPROVAL, expected);
      assert.equal(runtime.configOptions.find(o => o.id === 'pi_approval').current_value, expected);
      assert.equal(runtime.session, '/native/session.jsonl');
    } finally { await runtime.close(); }
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

test('should steer only the intended active Pi turn without aborting or starting another prompt', async () => {
  const fixture = piFixture();
  const runtime = await openPi({ cwd: process.cwd() }, () => {}, fixture);
  try {
    await runtime.prompt('original', 'turn1');
    await runtime.steer('turn1', 'new guidance');
    await assert.rejects(runtime.steer('turn2', 'wrong target'), /no longer the active turn/);
    fixture.emit({ type: 'agent_end' });
    await assert.rejects(runtime.steer('turn1', 'too late'), /no longer the active turn/);
    assert.deepEqual(fixture.commands.filter(c => ['prompt', 'steer', 'abort'].includes(c.type))
      .map(c => [c.type, c.message]), [['prompt', 'original'], ['steer', 'new guidance']]);
  } finally { await runtime.close(); }
});

test('should deliver both native images in attachment order with the file prompt', async () => {
  const f = piFixture(); const runtime = await openPi({ cwd: process.cwd() }, () => {}, f);
  const images = [{ mimeType: 'image/png', data: 'AQ==' }, { mimeType: 'image/jpeg', data: 'Ag==' }];
  try {
    await runtime.prompt('review report.zip', 'turn', images);
    assert.deepEqual(f.commands.find(c => c.type === 'prompt').images, images.map(image => ({ type: 'image', ...image })));
  } finally { await runtime.close(); }
});

test('should route a Pi extension turn after settlement without another prompt', async () => {
  const f = piFixture(), events = [];
  const runtime = await openPi({ cwd: process.cwd() }, event => events.push(event), f);
  try {
    f.emit({ type: 'agent_start' });
    await runtime.prompt('task', 'first');
    f.emit({ type: 'agent_end', messages: [], willRetry: false });
    f.emit({ type: 'agent_settled' });
    f.emit({ type: 'message_update', assistantMessageEvent: { type: 'text_delta', delta: 'late' } });
    f.emit({ type: 'agent_start' });
    f.emit({ type: 'message_update', assistantMessageEvent: { type: 'text_delta', delta: 'finished' } });
    f.emit({ type: 'agent_end', messages: [], willRetry: false });
    f.emit({ type: 'agent_settled' });
    assert.deepEqual(events.map(e => e.type), ['done', 'wake', 'text', 'done']);
    const wake = events[1];
    assert.equal(wake.after, 'first');
    assert.equal(wake.session, runtime.session);
    assert.notEqual(wake.turn, 'first');
    assert.equal(events[2].text, 'finished');
    assert.equal(events[2].turn, wake.turn);
    assert.equal(events[3].turn, wake.turn);
    assert.equal(f.commands.filter(c => c.type === 'prompt').length, 1);
    await runtime.cancel();
    f.emit({ type: 'agent_start' });
    assert.equal(events.length, 4);
  } finally { await runtime.close(); }
});

test('should deliver a concurrent user prompt after an unsolicited Pi turn on the bridge', { timeout: 10000 }, async t => {
  const cwd = await mkdtemp(join(tmpdir(), 'aipp-pi-wake-'));
  const peer = join(cwd, 'peer.mjs');
  await writeFile(peer, `
    import { createInterface } from 'node:readline';
    const send = event => console.log(JSON.stringify(event));
    console.error('AIPP_PI_EXTENSION_READY');
    for await (const line of createInterface({ input: process.stdin })) {
      const cmd = JSON.parse(line);
      send({ type: 'response', id: cmd.id, success: true,
        data: cmd.type === 'get_state' ? { sessionFile: 'session' } : {} });
      if (cmd.type === 'prompt') {
        send({ type: 'agent_end', messages: [{ role: 'assistant', content: cmd.message }] });
        if (cmd.message === 'first') send({ type: 'agent_start' });
      } else if (cmd.type === 'steer') {
        send({ type: 'agent_end', messages: [{ role: 'assistant', content: 'background' }] });
      }
    }
  `);
  const bridge = fork(new URL('./bridge.mjs', import.meta.url), [], { silent: true, execArgv: [] });
  const exited = new Promise(resolve => bridge.once('exit', resolve));
  t.after(async () => {
    bridge.stdin.end();
    await exited;
    await rm(cwd, { recursive: true, force: true });
  });
  const frames = createInterface({ input: bridge.stdout })[Symbol.asyncIterator]();
  const send = frame => bridge.stdin.write(JSON.stringify(frame) + '\n');
  const seen = [];
  const readUntil = async predicate => {
    for (;;) {
      const next = await frames.next();
      assert.ok(!next.done, 'bridge closed before the expected event');
      const frame = JSON.parse(next.value);
      assert.ok(frame.type !== 'fatal' && frame.ok !== false, frame.error);
      seen.push(frame);
      if (predicate(frame)) return frame;
    }
  };
  send({ type: 'init', id: 'init', driver: 'pi', config: { cwd, command: peer } });
  await readUntil(f => f.id === 'init');
  send({ type: 'prompt', id: 'p1', turn: '1', text: 'first' });
  const wake = await readUntil(f => f.type === 'wake');
  send({ type: 'prompt', id: 'p2', turn: '2', text: 'second' });
  send({ type: 'steer', id: 'steer', turn: wake.turn, text: 'finish' });
  await readUntil(f => f.type === 'done' && f.turn === '2');
  assert.deepEqual(seen.filter(f => f.type === 'done').map(f => [f.turn, f.final_text]),
    [['1', 'first'], [wake.turn, 'background'], ['2', 'second']]);
  assert.ok(seen.findIndex(f => f.type === 'prompt_started' && f.turn === '2')
    > seen.findIndex(f => f.type === 'done' && f.turn === wake.turn));
});
