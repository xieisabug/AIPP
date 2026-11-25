// Rust drives the real message queue. These in-memory native peers observe the
// permission on the actual ACP/Pi request, SDK send, or OpenCode HTTP request.
import { createInterface } from 'node:readline';
import { openPi } from './pi.mjs';
import { openCursor } from './cursor.mjs';
import { openOpenCode } from './opencode.mjs';
import { acpFixture, select } from './acp-native-test-helpers.mjs';
import { antigravityLaunch } from './acp-native.mjs';
import { piFixture, openCodeFixture, tick } from './test-helpers.mjs';

const driver = process.argv[2];
const incoming = createInterface({ input: process.stdin })[Symbol.asyncIterator]();
const read = async () => JSON.parse((await incoming.next()).value);
const report = value => process.stdout.write(JSON.stringify(value) + '\n');
let changeRejected = false;
const sent = [];

async function open(config, restore) {
  if (driver === 'kiro' || driver === 'antigravity') {
    const key = driver === 'kiro' ? 'autopilot' : 'mode';
    const options = [select(key, driver === 'kiro' ? ['off', 'on'] : ['default', 'auto', 'plan'])];
    const f = acpFixture({ options, respond(frame) {
      if (frame.method === 'session/set_config_option' && changeRejected)
        return { error: { code: -32602, message: 'fixture rejected permission' } };
      if (frame.method === 'session/prompt')
        sent.push([frame.params.sessionId, frame.params.prompt[0].text,
          f.launches[0][1].includes('--dangerously-skip-permissions') ? 'yolo' : options[0].currentValue]);
    } });
    const runtime = await f.open(driver, config, { antigravityLaunch: cfg =>
      antigravityLaunch({ ...cfg, profile: process.cwd() }, {}, {
        mkdir: async () => {}, writeFile: async () => {}, mkdtemp: async path => path, rm: async () => {},
      }) });
    if (restore && !f.commands.some(c => c.method === 'session/resume' && c.params.sessionId === restore)) throw Error('ACP did not resume the original session');
    let pending;
    return { runtime, async start(text) { pending = runtime.prompt(text, text); },
      async finish() { f.finish(); await pending; } };
  }
  if (driver === 'pi') {
    const f = piFixture({ rejectRestore: changeRejected });
    const runtime = await openPi(config, () => {}, f);
    if (restore && !f.commands.some(c => c.type === 'switch_session' && c.sessionPath === restore)) throw Error('Pi did not restore the original session');
    return { runtime, async start(text) {
      await runtime.prompt(text, text);
      const prompt = f.commands.findLast(c => c.type === 'prompt');
      sent.push([runtime.session, prompt.message, f.launches[0][2].env.AIPP_PI_APPROVAL]);
    }, async finish() { f.emit({ type: 'agent_end', messages: [] }); } };
  }
  if (driver === 'cursor') {
    let complete;
    const agent = (id, options) => ({ agentId: id, async close() {}, async send(text) {
      sent.push([id, text, options.local.autoReview ? 'on' : 'off']);
      return { wait: () => new Promise(resolve => { complete = resolve; }) };
    } });
    const sdk = { Agent: { create(options) {
      if (restore) throw Error('Cursor created a replacement session');
      return agent('s1', options);
    }, resume(id, options) {
      if (changeRejected) throw Error('fixture rejected resume');
      return agent(id, options);
    } } };
    const runtime = await openCursor(config, () => {}, { sdk });
    return { runtime, start: text => runtime.prompt(text, text), async finish() {
      complete({ status: 'finished', result: 'done' }); await tick();
    } };
  }
  if (driver !== 'opencode') throw Error('unknown fixture driver: ' + driver);
  const f = openCodeFixture({ respond(path, options) {
    const rule = JSON.parse(f.launches[0][2].env.OPENCODE_PERMISSION || '{}')['*'] ?? 'default';
    if (path === '/config') return Response.json({ permission: changeRejected ? 'allow' : rule });
    if (path.endsWith('/prompt_async')) sent.push([path.split('/')[2], JSON.parse(options.body).parts[0].text, rule]);
  } });
  const runtime = await openOpenCode(config, () => {}, f);
  if (restore && !f.requests.some(r => r.path === '/session/' + restore && r.method === 'GET')) throw Error('OpenCode did not restore the original session');
  return { runtime, start: text => runtime.prompt(text, text), async finish() {
    f.event('session.status', { sessionID: runtime.session, status: { type: 'busy' } });
    f.event('session.idle', { sessionID: runtime.session }); await tick();
  } };
}

let peer;
try {
  const first = await read();
  peer = await open(first.config);
  await peer.start(first.text);
  report({ session: peer.runtime.session, sent });
  if ((await read()).type !== 'finish') throw Error('expected first turn release');
  await peer.finish();
  report({ done: true, sent });
  const second = await read();
  changeRejected = second.reject;
  try {
    if (second.restart) {
      const original = peer.runtime.session;
      await peer.runtime.close();
      peer = undefined;
      peer = await open(second.config, original);
    } else {
      await peer.runtime.configure(...second.control);
    }
    await peer.start(second.text);
    await peer.finish();
    report({ sent });
  } catch (error) {
    report({ error: error.message, sent });
    if (!second.reject) process.exitCode = 1;
  }
} finally {
  await peer?.runtime.close();
  process.stdin.destroy();
}
