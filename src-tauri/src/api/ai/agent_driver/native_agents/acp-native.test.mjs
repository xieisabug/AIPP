import test from 'node:test';
import assert from 'node:assert/strict';
import { resolve } from 'node:path';
import { openAcpNative, antigravityLaunch, authorizationUrl } from './acp-native.mjs';
import { processFixture, tick } from './test-helpers.mjs';

const select = (id, values, currentValue = values[0]) => ({ id, name: id, category: id, type: 'select', currentValue, options: values.map(value => ({ value, name: value })) });
function fixture({ respond, options = [select('model', ['auto', 'model-a']), select('mode', ['default', 'review']), select('autopilot', ['on', 'off']), select('effortLevel', ['low', 'high'])], capabilities = { loadSession: true, sessionCapabilities: { resume: {} } } } = {}) {
  const commands = [], events = [], launches = [];
  let prompt, cleaned = false;
  const write = frame => child.stdout.write(`${JSON.stringify({ jsonrpc: '2.0', ...frame })}\n`);
  const child = processFixture(frame => {
    commands.push(frame);
    if (!frame.method || frame.id === undefined) return;
    const custom = respond?.(frame, { write, child });
    if (custom === false) return;
    if (custom) { write({ id: frame.id, ...custom }); return; }
    let result = {};
    if (frame.method === 'initialize') result = { protocolVersion: 1, agentCapabilities: capabilities, authMethods: [{ id: 'oauth-personal' }, { id: 'gemini-api-key' }] };
    if (['session/new', 'session/resume', 'session/load'].includes(frame.method)) result = { sessionId: 's1', configOptions: options };
    if (frame.method === 'session/set_config_option') {
      const option = options.find(option => option.id === frame.params.configId);
      if (option) option.currentValue = frame.params.value;
      result = { configOptions: options };
    }
    if (frame.method === 'session/prompt') { prompt = frame; return; }
    write({ id: frame.id, result });
  });
  return { child, commands, events, launches,
    launch: (...args) => { launches.push(args); return child; },
    antigravityLaunch: async () => ({ command: 'agy', args: ['acp'], env: {}, cleanup: async () => { cleaned = true; } }),
    get cleaned() { return cleaned; },
    update: update => write({ method: 'session/update', params: { sessionId: 's1', update } }),
    permission: (id, toolCall, options) => write({ id, method: 'session/request_permission', params: { sessionId: 's1', toolCall, options } }),
    finish: (stopReason = 'end_turn') => write({ id: prompt.id, result: { stopReason } }),
    open: (driver, config = {}) => openAcpNative(driver, { cwd: process.cwd(), ...config }, event => events.push(event), {
      launch: (...args) => { launches.push(args); return child; },
      antigravityLaunch: async () => ({ command: 'agy', args: ['acp'], env: {}, cleanup: async () => { cleaned = true; } }),
    }),
  };
}

test('should use Kiro V3 cli auth and disable Autopilot before selecting agent and model', async () => {
  const f = fixture(); const runtime = await f.open('kiro', { kiro_agent: 'review', model: 'model-a', kiro_effort: 'high' });
  try {
    assert.deepEqual(f.launches[0].slice(0, 2), ['kiro-cli', ['acp', '--agent-engine=v3', '--auth-method=cli']]);
    assert.deepEqual(f.commands.filter(c => c.method === 'session/set_config_option').map(c => [c.params.configId, c.params.value]), [['autopilot', 'off'], ['mode', 'review'], ['model', 'model-a'], ['effortLevel', 'high']]);
    assert.equal(runtime.session, 's1');
  } finally { await runtime.close(); }
});

for (const driver of ['kiro', 'antigravity']) {
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
      f.finish('cancelled'); await prompt;
      assert.equal(f.events.find(e => e.type === 'text').text, '答案\u2028next');
      assert.equal(f.events.find(e => e.status === 'failed').output, '"permission denied"');
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
      const runtime = await opening; await runtime.close(); assert.equal(f.cleaned, true);
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

test('should name the advertised Antigravity modes when the chosen one is missing', async () => {
  const f = fixture({ respond: c => c.method === 'session/new' ? { result: { sessionId: 's1', configOptions: [], modes: { availableModes: [{ id: 'default' }, { id: 'plan' }] } } } : undefined });
  await assert.rejects(f.open('antigravity', { antigravity_mode: 'yolo' }), /did not advertise the yolo permission mode \(advertised: default, plan\)/);
  await assert.rejects(fixture().open('antigravity', { antigravity_mode: 'turbo' }), /Unsupported Antigravity permission mode 'turbo'/);
});
