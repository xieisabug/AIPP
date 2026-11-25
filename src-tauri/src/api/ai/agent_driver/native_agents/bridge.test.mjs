import assert from 'node:assert/strict';
import { fork } from 'node:child_process';
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createInterface } from 'node:readline';
import test from 'node:test';

test('should report a busy config rejection on the bridge and finish both turns in one Cursor session', { timeout: 10000 }, async t => {
  const cwd = await mkdtemp(join(tmpdir(), 'aipp-bridge-config-'));
  const sdk = join(cwd, 'node_modules', '@cursor', 'sdk');
  await mkdir(sdk, { recursive: true });
  await writeFile(join(sdk, 'index.js'), `
    exports.Agent = { async create() {
      return { agentId: 'cursor-session', async close() {}, async send(text, options) {
        const result = text + ':' + options.model.id;
        return { wait: () => new Promise(resolve => {
          process.once('message', () => resolve({ status: 'finished', result }));
        }) };
      } };
    } };
  `);
  const bridge = fork(new URL('./bridge.mjs', import.meta.url), [], { silent: true, execArgv: [] });
  const exited = new Promise(resolve => bridge.once('exit', resolve));
  t.after(async () => {
    if (bridge.exitCode === null && bridge.signalCode === null) bridge.kill();
    await exited;
    await rm(cwd, { recursive: true, force: true });
  });
  const frames = createInterface({ input: bridge.stdout })[Symbol.asyncIterator]();
  const send = frame => bridge.stdin.write(JSON.stringify(frame) + '\n');
  const read = async () => JSON.parse((await frames.next()).value);
  send({ type: 'init', id: 'init', driver: 'cursor', config: { cwd, model: 'original' } });
  assert.equal((await read()).data.session, 'cursor-session');
  send({ type: 'prompt', id: 'p1', turn: '1', text: 'first' });
  assert.equal((await read()).ok, true);
  send({ type: 'config', id: 'busy', key: 'model', value: 'updated' });
  const rejected = await read();
  assert.equal(rejected.id, 'busy');
  assert.equal(rejected.ok, false);
  assert.equal(rejected.config_rejected, true);
  assert.match(rejected.error, /during a turn/);
  bridge.send('finish');
  assert.equal((await read()).text, 'first:original');
  assert.equal((await read()).type, 'done');
  send({ type: 'config', id: 'retry', key: 'model', value: 'updated' });
  assert.equal((await read()).ok, true);
  send({ type: 'prompt', id: 'p2', turn: '2', text: 'second' });
  assert.equal((await read()).ok, true);
  bridge.send('finish');
  assert.equal((await read()).text, 'second:updated');
  assert.equal((await read()).turn, '2');
});
