import test from 'node:test';
import assert from 'node:assert/strict';
import { resolve } from 'node:path';
import { antigravityLaunch, antigravitySignedIn, authorizationUrl } from './acp-native.mjs';
import { RequestRejected } from './common.mjs';
import { tick } from './test-helpers.mjs';

import { acpFixture as fixture, select } from './acp-native-test-helpers.mjs';

test('should use Kiro V3 cli auth and disable Autopilot before selecting agent and model', async () => {
  const f = fixture(); const runtime = await f.open('kiro', { kiro_agent: 'review', model: 'model-a', kiro_effort: 'high' });
  try {
    assert.deepEqual(f.launches[0].slice(0, 2), ['kiro-cli', ['acp', '--agent-engine=v3', '--auth-method=cli']]);
    assert.deepEqual(f.commands.filter(c => c.method === 'session/set_config_option').map(c => [c.params.configId, c.params.value]), [['autopilot', 'off'], ['mode', 'review'], ['model', 'model-a'], ['effortLevel', 'high']]);
    assert.equal(runtime.session, 's1');
  } finally { await runtime.close(); }
});

for (const driver of ['kiro', 'antigravity']) {
  test(`should apply the selected ${driver} permission before the second prompt in the same session`, async () => {
    const f = fixture({ options: [select('mode', ['default', 'auto']), select('autopilot', ['on', 'off'])] });
    const runtime = await f.open(driver);
    const [key, nativeKey, initial, next] = driver === 'kiro'
      ? ['autopilot', 'autopilot', 'off', 'on'] : ['antigravity_mode', 'mode', 'default', 'auto'];
    try {
      const first = runtime.prompt('first', 'turn-1');
      assert.equal(runtime.configOptions.find(o => o.id === key).current_value, initial);
      f.finish();
      await first;
      await runtime.configure(key, next);
      assert.equal(runtime.configOptions.find(o => o.id === key).current_value, next);
      const second = runtime.prompt('second', 'turn-2');
      f.finish();
      await second;
      assert.deepEqual(f.commands.filter(c => c.method === 'session/prompt' || c.params?.configId === nativeKey)
        .map(c => [c.params.sessionId, c.params.value ?? 'prompt']), [
        ['s1', initial], ['s1', 'prompt'], ['s1', next], ['s1', 'prompt'],
      ]);
      assert.equal(f.launches.length, 1);
    } finally { await runtime.close(); }
  });

  test(`should keep the ${driver} turn running after a busy config rejection and accept an idle retry`, async () => {
    const f = fixture();
    const runtime = await f.open(driver);
    try {
      const prompt = runtime.prompt('first', 'turn-1');
      await assert.rejects(runtime.configure('model', 'model-a'), error =>
        error instanceof RequestRejected && /configuration during a turn/.test(error.message));
      assert.equal(runtime.configOptions.find(option => option.id === 'model').current_value, 'auto');
      f.update({ sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: 'still running' } });
      f.finish();
      await prompt;
      assert.equal(f.events.find(event => event.type === 'text' && event.turn === 'turn-1').text, 'still running');
      await runtime.configure('model', 'model-a');
      assert.equal(runtime.configOptions.find(option => option.id === 'model').current_value, 'model-a');
      const next = runtime.prompt('second', 'turn-2');
      f.finish();
      await next;
      assert.deepEqual(f.commands.filter(c => c.method === 'session/prompt').map(c => c.params.sessionId), ['s1', 's1']);
      assert.equal(f.launches.length, 1);
    } finally { await runtime.close(); }
  });

  test(`should fail ${driver} restoration without creating a new session`, async () => {
    const f = fixture({ respond: c => c.method === 'session/resume' ? { error: { code: -32000, message: 'session missing' } } : undefined });
    await assert.rejects(f.open(driver, { session: 'saved' }), /session\/resume.*session missing/);
    assert.equal(f.commands.some(c => c.method === 'session/new' || c.method === 'session/load'), false);
    assert.equal(f.child.killed, true);
  });
  test(`should stream ${driver} text and tool updates and settle only on the prompt response`, async () => {
    const f = fixture(); const runtime = await f.open(driver);
    try {
      const prompt = runtime.prompt('hello', 'turn');
      f.update({ sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: '答案\u2028next' } });
      f.update({ sessionUpdate: 'tool_call', toolCallId: 't1', title: 'Read file', status: 'in_progress', rawInput: { path: 'a' } });
      f.update({ sessionUpdate: 'tool_call_update', toolCallId: 't1', status: 'failed', rawOutput: 'permission denied' });
      await runtime.cancel();
      assert.equal(f.events.some(e => e.type === 'done'), false);
      assert.equal(f.commands.filter(c => c.method === 'session/cancel').length, 1);
      const usage = { totalTokens: 20, inputTokens: 14, outputTokens: 6, cachedReadTokens: 10 };
      f.update({ sessionUpdate: 'usage_update', used: 1000, size: 10000 });
      f.finish('cancelled', usage); await prompt;
      assert.deepEqual(f.events.filter(e => e.type === 'usage'), [{ type: 'usage', turn: 'turn', usage }]);
      assert.equal(f.events.find(e => e.type === 'text').text, '答案\u2028next');
      assert.equal(f.events.find(e => e.status === 'failed').output, 'permission denied');
      assert.equal(f.events.at(-1).cancelled, true);
    } finally { await runtime.close(); }
  });
  test(`should surface ${driver} process failure with exit status`, async () => {
    const f = fixture(); const runtime = await f.open(driver);
    try {
      const prompt = runtime.prompt('hello', 'turn');
      f.child.emit('exit', 42, null);
      await assert.rejects(prompt, /code=42/);
      assert.match(f.events.find(e => e.type === 'fatal').error, /code=42/);
    } finally { await runtime.close(); }
  });
}

test('should preserve Kiro loadSession when resume is not advertised', async () => {
  const f = fixture({ capabilities: { loadSession: true } }); const runtime = await f.open('kiro', { session: 's1' });
  try { assert.equal(f.commands.some(c => c.method === 'session/load'), true); }
  finally { await runtime.close(); }
});

test('should distinguish a native Kiro config rejection from a malformed acknowledgement', async () => {
  const f = fixture({ respond(frame) {
    if (frame.method !== 'session/set_config_option' || frame.params.configId !== 'model') return;
    if (frame.params.value === 'unknown') return { error: { code: -32602, message: 'unknown model' } };
    return {};
  } });
  const runtime = await f.open('kiro');
  try {
    await assert.rejects(runtime.configure('model', 'unknown'), error =>
      error instanceof RequestRejected && /unknown model/.test(error.message));
    await assert.rejects(runtime.configure('model', 'model-a'), error =>
      !(error instanceof RequestRejected) && /omitted result\/error/.test(error.message));
  } finally { await runtime.close(); }
});

for (const [capabilities, canLoad, canResume] of [
  [{}, false, false],
  [{ loadSession: true }, true, false],
  [{ sessionCapabilities: { resume: {} } }, false, true],
]) {
  test(`should report Kiro handshake support with load=${canLoad} and resume=${canResume}`, async () => {
    const runtime = await fixture({ capabilities }).open('kiro');
    try { assert.deepEqual([runtime.canLoad, runtime.canResume], [canLoad, canResume]); }
    finally { await runtime.close(); }
  });
}

test('should wait for Kiro prompt activity before sending an early cancel', async () => {
  const f = fixture(); const runtime = await f.open('kiro');
  try {
    const prompt = runtime.prompt('task', 'turn'); await runtime.cancel();
    assert.equal(f.commands.some(c => c.method === 'session/cancel'), false);
    f.update({ sessionUpdate: 'agent_thought_chunk', content: { type: 'text', text: 'start' } });
    assert.equal(f.commands.filter(c => c.method === 'session/cancel').length, 1);
    f.finish('cancelled'); await prompt;
  } finally { await runtime.close(); }
});

test('should identify the Antigravity message permission from the native dispatch fields', async () => {
  const f = fixture();
  const runtime = await f.open('antigravity', { mcp: { command: 'fixture', args: [] } });
  try {
    const prompt = runtime.prompt('send an internal message', 'turn');
    const args = { message: 'ACK', toolSummary: 'Local diagnostic echo call' };
    const name = 'mcp_aipp_aipp_t61_message_bot';
    const call = { sessionUpdate: 'tool_call', toolCallId: 'call_123', kind: 'other',
      name, args, rawInput: args, title: `Run ${name}` };
    let id = 80;
    const request = fields => {
      f.permission(++id, { ...call, ...fields }, [{ optionId: 'once', name: 'Allow', kind: 'allow_once' }]);
      return f.events.at(-1);
    };
    const permission = request({});
    assert.equal(permission.type, 'permission');
    assert.deepEqual(permission.toolMeta, { mcp: { serverName: 'aipp', toolName: 'aipp_t61_message_bot' } });
    assert.deepEqual(permission.input, args);
    assert.equal(request({ name: undefined }).toolMeta, undefined);
    assert.equal(request({ name: 'mcp_external_aipp_t61_message_bot' }).toolMeta, undefined);
    assert.equal(request({ name: 'run_command', rawInput: { name, ...args } }).toolMeta, undefined);
    assert.equal(request({ args: { message: 'different' } }).toolMeta, undefined);
    const external = { mcp: { serverName: 'external', toolName: 'aipp_t61_message_bot' } };
    assert.deepEqual(request({ _meta: external }).toolMeta, external);
    f.finish(); await prompt;
  } finally { await runtime.close(); }
});

test('should never offer persistent Kiro approval when an allow_once option exists', async () => {
  const f = fixture(); const runtime = await f.open('kiro');
  try {
    const prompt = runtime.prompt('task', 'turn');
    f.permission(80, { toolCallId: 't', title: 'shell' }, [{ optionId: 'once', name: 'Allow', kind: 'allow_once' }, { optionId: 'forever', name: 'Always', kind: 'allow_always' }]);
    const permission = f.events.find(e => e.type === 'permission');
    assert.equal(permission.options.some(o => o.kind === 'allow_always'), false);
    await assert.rejects(runtime.reply('80', 'forever'), /not offered/);
    await runtime.reply('80', 'once');
    assert.deepEqual(f.commands.find(c => c.id === 80).result, { outcome: { outcome: 'selected', optionId: 'once' } });
    await assert.rejects(runtime.reply('80', 'once'), /expired/);
    f.finish(); await prompt;
  } finally { await runtime.close(); }
});

test('should reject unavailable Kiro agent and effort without ignoring the selection', async () => {
  for (const config of [{ kiro_agent: 'missing' }, { kiro_effort: 'unsupported' }]) {
    const f = fixture(); await assert.rejects(f.open('kiro', config), /unavailable configuration/);
    assert.equal(f.child.killed, true);
  }
});

test('should discover Kiro model options arriving after session initialization', async () => {
  const f = fixture({ options: [select('autopilot', ['on', 'off'])] });
  const opening = f.open('kiro', { probe: true });
  await tick(); f.update({ sessionUpdate: 'config_option_update', configOptions: [select('model', ['late-model'])] });
  const runtime = await opening;
  try { assert.deepEqual(await runtime.models(), [{ id: 'late-model', name: 'late-model' }]); }
  finally { await runtime.close(); }
});

test('should use truthful client identity and native MCP with Antigravity read/write/terminal disabled', async () => {
  const f = fixture(); const mcp = { command: 'aipp', args: ['--mcp'], env: { TOKEN: 'synthetic' } };
  const runtime = await f.open('antigravity', { mcp });
  try {
    const init = f.commands.find(c => c.method === 'initialize');
    assert.equal(init.params.clientInfo.name, 'aipp');
    assert.deepEqual(init.params.clientCapabilities, { fs: { readTextFile: false, writeTextFile: false }, terminal: false });
    assert.equal(f.commands[1].method, 'authenticate');
    assert.deepEqual(f.commands.find(c => c.method === 'session/new').params.mcpServers, [{ name: 'aipp', command: 'aipp', args: ['--mcp'], env: [{ name: 'TOKEN', value: 'synthetic' }] }]);
  } finally { await runtime.close(); }
});

test('should reject an unavailable Antigravity model without falling back', async () => {
  const f = fixture(); await assert.rejects(f.open('antigravity', { model: 'unavailable' }), /unavailable configuration model=unavailable/);
  assert.equal(f.commands.some(c => c.method === 'session/prompt'), false);
  const grouped = { ...select('model', []), options: [{ name: 'account models', options: [{ value: 'saved-model', name: 'Saved model' }] }] };
  const restored = fixture({ options: [grouped, select('mode', ['default'])] });
  const runtime = await restored.open('antigravity', { session: 's1', model: 'saved-model' });
  try {
    assert.deepEqual(await runtime.models(), [{ id: 'saved-model', name: 'Saved model' }]);
    assert.equal(restored.commands.find(c => c.params?.configId === 'model').params.value, 'saved-model');
  } finally { await runtime.close(); }
});

test('should map Antigravity interaction choices to questions and native option IDs', async () => {
  const f = fixture(); const runtime = await f.open('antigravity');
  try {
    const prompt = runtime.prompt('task', 'turn');
    f.permission(81, { toolCallId: 'interaction_question', title: 'Choose approach' }, [{ optionId: 'choice-id', name: 'A', kind: 'allow_once' }, { optionId: 'reject-id', name: 'B', kind: 'reject_once' }]);
    const question = f.events.find(e => e.type === 'question');
    assert.equal(f.events.some(e => e.type === 'permission'), false);
    await assert.rejects(runtime.answer('81', { 'Choose approach': 'invented' }), /offered choice/);
    await runtime.answer('81', { 'Choose approach': question.questions[0].options[0].label });
    assert.equal(f.commands.find(c => c.id === 81).result.outcome.optionId, 'choice-id');
    f.permission(82, { toolCallId: 'interaction_question2', title: 'Choose again' }, [{ optionId: 'a', name: 'A', kind: 'allow_once' }, { optionId: 'b', name: 'B', kind: 'reject_once' }]);
    await runtime.answer('82', { 'Choose again': 'B [b]' });
    assert.deepEqual(f.commands.find(c => c.id === 82).result.outcome, { outcome: 'selected', optionId: 'b' });
    f.finish(); await prompt;
  } finally { await runtime.close(); }
});

test('should fail ambiguous Antigravity interaction IDs instead of consuming a different option', async () => {
  const f = fixture(); const runtime = await f.open('antigravity');
  try {
    const prompt = runtime.prompt('task', 'turn');
    f.permission(83, { toolCallId: 'interaction_bad', title: 'Choose' }, [{ optionId: 'duplicate', name: 'A', kind: 'allow_once' }, { optionId: 'duplicate', name: 'B', kind: 'reject_once' }]);
    await assert.rejects(prompt, /invalid or duplicate/);
    assert.equal(f.events.some(e => e.type === 'question'), false);
  } finally { await runtime.close(); }
});

const url = 'https://accounts.google.com/o/oauth2/v2/auth?state=synthetic&response_type=code&redirect_uri=http%3A%2F%2F127.0.0.1%3A3456%2F';
test('should check sign-in without creating a session and distinguish an auth challenge from a runtime failure', async () => {
  for (const outcome of ['ok', 'challenge', 'auth_required', 'failure']) {
    const f = fixture({ respond(frame, { child }) {
      if (frame.method !== 'authenticate' || outcome === 'ok') return;
      if (outcome === 'challenge') {
        child.stdout.write(`Open the following link to authenticate the ACP server: ${url}\n`);
        return false;
      }
      return { error: { code: outcome === 'auth_required' ? -32000 : -32603, message: 'native diagnostic' } };
    } });
    const check = antigravitySignedIn({ cwd: process.cwd() }, f);
    if (outcome === 'failure') await assert.rejects(check, /native diagnostic/);
    else assert.equal(await check, outcome === 'ok');
    assert.deepEqual(f.commands.map(cmd => cmd.method), ['initialize', 'authenticate']);
    assert.equal(f.cleaned, true);
  }
});
test('should validate Antigravity OAuth origin, callback and state', () => {
  assert.equal(authorizationUrl(url), url);
  for (const value of [url.replace('accounts.google.com', 'example.org'), url.replace('3456', '80'), `${url}&state=duplicate`, url.replace('127.0.0.1', 'example.org'), url.replace('synthetic', '')]) assert.throws(() => authorizationUrl(value));
});

test('should present an authorization URL only during explicit login and wait for authenticate completion', async () => {
  for (const login of [false, true]) {
    let authenticate;
    const f = fixture({ respond: (frame, { child }) => {
      if (frame.method !== 'authenticate') return;
      authenticate = frame;
      child.stdout.write(`Open the following link to authenticate the ACP server: ${url}\n`);
      return false;
    } });
    const opening = f.open('antigravity', { login });
    if (!login) { await assert.rejects(opening, /提供商设置/); assert.equal(f.events.length, 0); }
    else {
      await tick(); assert.equal(f.events[0].url, url);
      assert.equal(f.commands.some(c => c.method === 'session/new'), false);
      f.child.stdout.write(`${JSON.stringify({ jsonrpc: '2.0', id: authenticate.id, result: {} })}\n`);
      const runtime = await opening;
      assert.deepEqual(await runtime.models(), [{ id: 'auto', name: 'auto' }, { id: 'model-a', name: 'model-a' }]);
      assert.equal(f.commands.filter(c => c.method === 'session/new').length, 1);
      await runtime.close(); assert.equal(f.cleaned, true);
    }
  }
});

test('should isolate Antigravity credentials and clean only the owned temporary directory', async () => {
  const operations = [];
  const fs = { mkdir: async (...args) => operations.push(['mkdir', ...args]), writeFile: async (...args) => operations.push(['write', ...args]),
    mkdtemp: async path => `${path}owned`, rm: async (...args) => operations.push(['rm', ...args]) };
  const profile = resolve('synthetic-profile'), temp_root = resolve('synthetic-bridge');
  const plan = await antigravityLaunch({ profile, temp_root }, { GEMINI_API_KEY: 'wrong-account', GOOGLE_API_KEY: 'wrong-key', GEMINI_HOME: 'wrong-profile', PATH: 'keep' }, fs);
  assert.deepEqual(plan.args, ['acp']); assert.equal(plan.command, 'agy');
  assert.equal(plan.env.GEMINI_API_KEY, undefined); assert.equal(plan.env.GOOGLE_API_KEY, undefined);
  assert.equal(plan.env.GEMINI_HOME, profile); assert.equal(plan.env.PATH, 'keep');
  assert.equal(plan.env.AGY_ACP_FORCE_FILE_STORAGE, '1');
  assert.ok(plan.env.TEMP.startsWith(temp_root));
  assert.deepEqual(JSON.parse(operations.find(op => op[0] === 'write')[2]), { auth: { type: 'oauth-personal' } });
  await plan.cleanup(); assert.equal(operations.at(-1)[1], plan.env.TEMP);
  const keyPlan = await antigravityLaunch({ profile, temp_root, auth_method: 'gemini-api-key', auth_key: 'explicit', runtime: 'standalone', harness: resolve('harness'), command: resolve('antigravity') }, {}, fs);
  assert.equal(keyPlan.env.GEMINI_API_KEY, 'explicit'); assert.equal(keyPlan.env.ANTIGRAVITY_HARNESS_PATH, resolve('harness'));
  assert.deepEqual(keyPlan.args, process.platform === 'linux' ? ['--uid='] : []);
  await keyPlan.cleanup();
  await assert.rejects(antigravityLaunch({ profile, runtime: 'standalone' }, {}, fs), /matching localharness/);
  await assert.rejects(antigravityLaunch({ profile, auth_method: 'gemini-api-key' }, {}, fs), /key is missing/);
});

test('should keep Kiro Autopilot off by default and enable it only when the assistant opts in', async () => {
  const f = fixture(); const runtime = await f.open('kiro', { kiro_autopilot: 'on' });
  try {
    assert.deepEqual(f.commands.find(c => c.method === 'session/set_config_option').params, { sessionId: 's1', configId: 'autopilot', value: 'on' });
  } finally { await runtime.close(); }
  await assert.rejects(fixture().open('kiro', { kiro_autopilot: 'always' }), /Unsupported Kiro Autopilot setting 'always'/);
});

test('should apply the chosen Antigravity permission mode through its advertised alias', async () => {
  const modes = { availableModes: [{ id: 'default' }, { id: 'autoEdit' }, { id: 'yolo' }] };
  const f = fixture({ respond: c => c.method === 'session/new' ? { result: { sessionId: 's1', configOptions: [], modes } } : undefined });
  const runtime = await f.open('antigravity', { antigravity_mode: 'auto_edit' });
  try {
    assert.deepEqual(f.commands.find(c => c.method === 'session/set_mode').params, { sessionId: 's1', modeId: 'autoEdit' });
  } finally { await runtime.close(); }
});

test('should enable CLI auto-approval only for a yolo conversation', async () => {
  const fs = { mkdir: async () => {}, writeFile: async () => {}, mkdtemp: async path => path };
  const config = { profile: resolve('profile'), antigravity_mode: 'yolo' };
  for (const [overrides, expected] of [
    [{}, ['acp', '--dangerously-skip-permissions']],
    [{ probe: true }, ['acp']], [{ login: true }, ['acp']], [{ logout: true }, ['acp']],
    [{ antigravity_mode: 'auto' }, ['acp']],
    [{ runtime: 'standalone', harness: resolve('harness') }, process.platform === 'linux' ? ['--uid='] : []],
  ]) {
    const plan = await antigravityLaunch({ ...config, ...overrides }, {}, fs);
    assert.deepEqual(plan.args, expected);
  }
});

test('should apply Antigravity execution modes on new and resumed CLI sessions', async () => {
  for (const session of [undefined, 's1']) {
    for (const [setting, mode] of [['auto_edit', 'accept-edits'], ['auto', 'auto'], ['plan', 'plan'], ['yolo', 'default']]) {
      const f = fixture({ options: [select('mode', ['default', 'accept-edits', 'auto', 'plan'], 'plan')] });
      const runtime = await f.open('antigravity', { antigravity_mode: setting, session });
      try {
        assert.equal(f.commands.find(c => c.method === 'session/set_config_option').params.value, mode);
      } finally { await runtime.close(); }
    }
  }
});

test('should discover Antigravity account models independently of conversation permissions', async () => {
  for (const catalog of [
    { configOptions: [select('model', ['available'])] },
    { models: { availableModels: [{ modelId: 'available', name: 'available' }] } },
  ]) {
    const f = fixture({ respond: c => c.method === 'session/new' ? { result: { sessionId: 'probe', ...catalog } } : undefined });
    const runtime = await f.open('antigravity', { probe: true, antigravity_mode: 'invalid', model: 'stale' });
    try {
      assert.deepEqual(await runtime.models(), [{ id: 'available', name: 'available' }]);
      assert.deepEqual(f.commands.map(c => c.method), ['initialize', 'authenticate', 'session/new']);
    } finally { await runtime.close(); }
  }
});

test('should name the advertised Antigravity modes when the chosen one is missing', async () => {
  const f = fixture({ respond: c => c.method === 'session/new' ? { result: { sessionId: 's1', configOptions: [select('mode', ['default', 'plan'])], modes: { availableModes: [{ id: 'default' }, { id: 'plan' }] } } } : undefined });
  await assert.rejects(f.open('antigravity', { runtime: 'standalone', antigravity_mode: 'yolo' }), /did not advertise the yolo permission mode \(advertised: default, plan\)/);
  await assert.rejects(fixture().open('antigravity', { antigravity_mode: 'turbo' }), /Unsupported Antigravity permission mode 'turbo'/);
});

for (const driver of ['kiro', 'antigravity']) {
  test(`should separate ${driver} thoughts and retain typed tool metadata without ending a later thought on progress`, async () => {
    const f = fixture(); const runtime = await f.open(driver);
    try {
      const prompt = runtime.prompt('task', 'turn');
      f.update({ sessionUpdate: 'agent_thought_chunk', content: { type: 'text', text: 'before' } });
      f.update({ sessionUpdate: 'tool_call', toolCallId: 'read', kind: 'read', title: 'Read file', status: 'in_progress', rawInput: { path: 'a.rs' } });
      assert.equal(f.events.at(-1).metadata.native_kind, 'read');
      f.update({ sessionUpdate: 'agent_thought_chunk', content: { type: 'text', text: 'after' } });
      f.update({ sessionUpdate: 'tool_call_update', toolCallId: 'read', status: 'completed', rawOutput: [{ type: 'text', text: 'file text' }] });
      assert.equal(f.events.at(-1).output, 'file text');
      assert.deepEqual(f.events.at(-1).metadata.rawResult, [{ type: 'text', text: 'file text' }]);
      f.update({ sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: 'answer' } });
      const thoughts = f.events.filter(e => e.kind === 'reasoning');
      assert.deepEqual(thoughts.map(e => e.status), ['running', 'success', 'running', 'success']);
      assert.notEqual(thoughts[0].id, thoughts[2].id);
      assert.equal(thoughts[0].id, thoughts[1].id);
      assert.equal(thoughts[2].id, thoughts[3].id);
      f.finish(); await prompt;
      assert.equal(f.events.at(-1).type, 'done');
    } finally { await runtime.close(); }
  });
}

for (const driver of ['kiro', 'antigravity']) {
  test(`should deliver Butler context once on restored ${driver} and preserve the next input`, async () => {
    const f = fixture();
    const runtime = await f.open(driver, { session: 's1', butlerInstructions: 'Butler role' });
    try {
      for (const text of ['first', 'second', '/compact', 'after']) {
        const pending = runtime.prompt(text, text);
        await tick();
        const frame = f.commands.filter(item => item.method === 'session/prompt').at(-1);
        assert.deepEqual(frame.params.prompt.map(item => item.text),
          ['first', 'after'].includes(text) ? [text, 'Butler role'] : [text]);
        f.finish(); await pending;
      }
    } finally { await runtime.close(); }
  });
}

test('should use the advertised Antigravity system append on resume without repeating it in user input', async () => {
  const f = fixture({ capabilities: { _meta: { antigravity: { systemPrompt: { append: true } } } } });
  const runtime = await f.open('antigravity', { session: 's1', butlerInstructions: 'role' });
  try {
    assert.equal(f.commands.find(c => c.method === 'session/resume').params._meta.antigravity.systemPrompt.append, 'role');
    const pending = runtime.prompt('hello', 1); await tick();
    assert.deepEqual(f.commands.at(-1).params.prompt, [{type:'text', text:'hello'}]);
    f.finish(); await pending;
  } finally { await runtime.close(); }
});

test('should log out through native ACP without authenticating or opening a session', async () => {
  const f = fixture({ capabilities: { auth: { logout: true } } });
  const runtime = await f.open('antigravity', { logout: true });
  try { assert.deepEqual(f.commands.map(c => c.method), ['initialize', 'logout']); }
  finally { await runtime.close(); }
});

test('should discover and switch models advertised through legacy ACP session models', async () => {
  const f = fixture({ respond: frame => frame.method === 'session/new' ? { result: {
    sessionId: 's1', modes: { availableModes: [{ id: 'default' }] },
    models: { currentModelId: 'a', availableModels: [{ modelId: 'a', name: 'A' }, { modelId: 'b', name: 'B' }] },
  } } : undefined });
  const runtime = await f.open('antigravity', { model: 'b' });
  try {
    assert.deepEqual(await runtime.models(), [{ id: 'a', name: 'A' }, { id: 'b', name: 'B' }]);
    assert.equal(runtime.configOptions.find(option => option.id === 'model').current_value, 'b');
    assert.deepEqual(f.commands.find(frame => frame.method === 'session/set_model').params, { sessionId: 's1', modelId: 'b' });
  } finally { await runtime.close(); }
});

test('should preserve a modern model option advertised while a legacy switch is in flight', async () => {
  const modern = select('model', ['b', 'c'], 'b');
  const f = fixture({
    options: [modern],
    respond(frame, { write }) {
      if (frame.method === 'session/new') return { result: {
        sessionId: 's1', modes: { availableModes: [{ id: 'default' }] },
        models: { currentModelId: 'a', availableModels: [{ modelId: 'a', name: 'A' }, { modelId: 'b', name: 'B' }] },
      } };
      if (frame.method === 'session/set_model') {
        write({ method: 'session/update', params: {
          sessionId: 's1', update: { sessionUpdate: 'config_option_update', configOptions: [modern] },
        } });
        return { result: {} };
      }
    },
  });
  const runtime = await f.open('antigravity');
  try {
    await runtime.configure('model', 'b');
    assert.deepEqual(await runtime.models(), [{ id: 'b', name: 'b' }, { id: 'c', name: 'c' }]);
    assert.equal(runtime.configOptions.find(option => option.id === 'model').current_value, 'b');
    await runtime.configure('model', 'c');
    assert.deepEqual(f.commands.filter(frame => frame.method.startsWith('session/set_')).map(frame => frame.method),
      ['session/set_mode', 'session/set_model', 'session/set_config_option']);
    assert.equal(runtime.configOptions.find(option => option.id === 'model').current_value, 'c');
  } finally { await runtime.close(); }
});

for (const driver of ['kiro', 'antigravity']) {
  test(`should send native images after ${driver} negotiates image inputs`, async () => {
    const f = fixture({ capabilities: { promptCapabilities: { image: true } } });
    const runtime = await f.open(driver);
    const image = { mimeType: 'image/png', data: 'AQ==' };
    try {
      const done = runtime.prompt('report.zip', 'turn', [image]);
      const blocks = f.commands.find(c => c.method === 'session/prompt').params.prompt;
      assert.deepEqual(blocks, [{ type: 'text', text: 'report.zip' }, { type: 'image', ...image }]);
      f.finish(); await done;
    } finally { await runtime.close(); }
  });
}

test('should send a PDF resource link when Antigravity advertises embedded context', async () => {
  const f = fixture({ capabilities: { promptCapabilities: { embeddedContext: true } } });
  const runtime = await f.open('antigravity');
  const path = process.cwd() + '/预算 2026.pdf';
  try {
    const done = runtime.prompt(path, 'turn', [], [path]);
    const blocks = f.commands.find(c => c.method === 'session/prompt').params.prompt;
    assert.equal(blocks[1].type, 'resource_link');
    assert.equal(blocks[1].name, '预算 2026.pdf');
    assert.equal(blocks[1].mimeType, 'application/pdf');
    assert.match(blocks[1].uri, /^file:.*%20/);
    f.finish(); await done;
  } finally { await runtime.close(); }
});

test('should retain a readable image path when Kiro has no image input and reject unresolved bytes', async () => {
  const f = fixture();
  const runtime = await f.open('kiro');
  const images = [{ mimeType: 'image/png', data: 'AQ==' }];
  try {
    await assert.rejects(runtime.prompt('image', 'first', images), /图片输入能力/);
    const done = runtime.prompt('local-image.png', 'second', images, [], true);
    assert.deepEqual(f.commands.at(-1).params.prompt, [{ type: 'text', text: 'local-image.png' }]);
    f.finish(); await done;
  } finally { await runtime.close(); }
});
