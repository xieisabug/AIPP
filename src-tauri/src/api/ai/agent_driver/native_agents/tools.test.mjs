import test from 'node:test';
import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import { PassThrough } from 'node:stream';
import { join } from 'node:path';
import { resolveCommand } from './common.mjs';
import { detect, npmCli, run } from './tools.mjs';

function fakeChild({ stdout = '', stderr = '', code = 0, hang = false } = {}) {
  const child = new EventEmitter();
  child.stdin = new PassThrough(); child.stdout = new PassThrough(); child.stderr = new PassThrough();
  child.kill = () => { child.emit('close', null, 'SIGTERM'); };
  if (!hang) queueMicrotask(() => {
    child.stdout.write(stdout); child.stderr.write(stderr);
    setImmediate(() => child.emit('close', code, null));
  });
  return child;
}

test('should resolve Windows commands in launch order when several shims share a PATH directory', () => {
  const files = new Set(['C:\\a\\pi.cmd', 'C:\\a\\pi.ps1', 'C:\\b\\pi.exe']);
  const exists = file => files.has(file.replaceAll('/', '\\'));
  assert.equal(resolveCommand('pi', 'win32', 'C:\\a;C:\\b', exists)?.replaceAll('/', '\\'), 'C:\\a\\pi.ps1');
  assert.equal(resolveCommand('missing', 'win32', 'C:\\a', exists), null);
});

test('should scan PATH on Unix and keep explicit paths when resolving commands', () => {
  const exists = file => file === join('/opt/b', 'opencode') || file === './bin/opencode';
  assert.equal(resolveCommand('opencode', 'linux', '/opt/a:/opt/b', exists), join('/opt/b', 'opencode'));
  assert.equal(resolveCommand('./bin/opencode', 'linux', '', exists), './bin/opencode');
  assert.equal(resolveCommand('opencode', 'linux', '', exists), null);
});

test('should report version output and per-command launch failures when detecting CLIs', async () => {
  const launches = [];
  const result = await detect([{ key: 'auto', command: 'pi' }, { key: 'shim', command: 'C:\\npm\\pi.cmd' }], {
    resolveCommand: command => command === 'pi' ? '/home/u/.pi/agent/bin/pi' : command,
    launchPlan: command => {
      if (command.endsWith('.cmd')) throw new Error(`Use the executable or JavaScript CLI entry point instead of a shell shim: ${command}`);
      return { file: command, args: ['--version'], resolved: command };
    },
    launch: (...args) => { launches.push(args); return fakeChild({ stdout: '1.5.1\n' }); },
  });
  assert.deepEqual(result[0], { key: 'auto', command: 'pi', resolved: '/home/u/.pi/agent/bin/pi', ok: true, code: 0, output: '1.5.1' });
  assert.equal(result[1].ok, false);
  assert.match(result[1].error, /shell shim/);
  assert.deepEqual(launches.map(([command, args]) => [command, args]), [['pi', ['--version']]]);
});

test('should keep a failed exit code distinct from success when detecting CLIs', async () => {
  const [result] = await detect([{ key: 'auto', command: 'agy' }], {
    resolveCommand: () => '/bin/agy', launchPlan: command => ({ file: command, args: [], resolved: command }),
    launch: () => fakeChild({ stderr: 'unknown flag', code: 2 }),
  });
  assert.equal(result.ok, false);
  assert.equal(result.code, 2);
  assert.equal(result.output, 'unknown flag');
});

test('should stream output and stop the process tree when a maintenance command times out', async () => {
  const chunks = [];
  const finished = await run('opencode', ['upgrade'], { start: () => fakeChild({ stdout: 'Upgrading…\n', code: 0 }), onOutput: text => chunks.push(text) });
  assert.equal(finished.code, 0);
  assert.equal(finished.output, 'Upgrading…\n');
  assert.deepEqual(chunks, ['Upgrading…\n']);
  let killed = false;
  const child = fakeChild({ hang: true }); const kill = child.kill; child.kill = () => { killed = true; kill(); };
  await assert.rejects(run('agy', ['update'], { start: () => child, timeout: 20 }), /timed out/);
  assert.equal(killed, true);
});

test('should locate npm beside Node for Windows and Unix layouts or fail with the Node path', () => {
  const windows = join('C:', 'nvm', 'v22', 'node_modules', 'npm', 'bin', 'npm-cli.js');
  assert.equal(npmCli(join('C:', 'nvm', 'v22', 'node.exe'), file => file === windows), windows);
  const unix = join('/opt/node/bin', '..', 'lib', 'node_modules', 'npm', 'bin', 'npm-cli.js');
  assert.equal(npmCli('/opt/node/bin/node', file => file === unix), unix);
  assert.throws(() => npmCli('/opt/bare/node', () => false), /npm was not found next to Node \/opt\/bare\/node/);
});
