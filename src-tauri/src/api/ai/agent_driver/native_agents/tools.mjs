// Environment detection and maintenance commands for the provider settings form.
// Everything goes through launchPlan()/launch(), so a detected path and version
// describe exactly the program a chat session would start with the same Node and env.
import { existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { deadline, launch, launchPlan, resolveCommand, stopChild } from './common.mjs';

const OUTPUT_LIMIT = 64 * 1024;

// Run a command to completion, forwarding output chunks; never throws for a non-zero exit.
export function run(command, args, { timeout = 20000, onOutput, start = launch } = {}) {
  let child;
  try { child = start(command, args, {}); } catch (error) { return Promise.reject(error); }
  child.stdin?.end();
  let output = '';
  const collect = chunk => {
    const text = chunk.toString();
    output = (output + text).slice(-OUTPUT_LIMIT);
    onOutput?.(text);
  };
  child.stdout?.on('data', collect);
  child.stderr?.on('data', collect);
  const finished = new Promise((resolve, reject) => {
    child.once('error', reject);
    child.once('close', (code, signal) => resolve({ code, signal, output }));
  });
  return deadline(finished, `${command} ${args.join(' ')}`, timeout).catch(async error => {
    await stopChild(child);
    throw error;
  });
}

// `--version` for each command. Failures are reported per command, never thrown.
export async function detect(commands, dependencies = {}) {
  const plan = dependencies.launchPlan || launchPlan;
  const resolve = dependencies.resolveCommand || resolveCommand;
  return Promise.all(commands.map(async ({ key, command }) => {
    const resolved = resolve(command);
    try {
      const target = plan(command, ['--version']);
      const result = await run(command, ['--version'], { start: dependencies.launch, timeout: dependencies.timeout });
      return { key, command, resolved: resolved ?? target.resolved, ok: result.code === 0, code: result.code, output: result.output.trim() };
    } catch (error) {
      return { key, command, resolved, ok: false, error: error.message };
    }
  }));
}

// npm shipped with the running Node: <dir>/node_modules/npm (Windows, official zips)
// or <prefix>/lib/node_modules/npm (Unix layouts).
export function npmCli(execPath = process.execPath, exists = existsSync) {
  const directory = dirname(execPath);
  const candidates = [join(directory, 'node_modules', 'npm', 'bin', 'npm-cli.js'),
    join(directory, '..', 'lib', 'node_modules', 'npm', 'bin', 'npm-cli.js')];
  const found = candidates.find(exists);
  if (!found) throw new Error(`npm was not found next to Node ${execPath}; choose a Node.js installation that includes npm`);
  return found;
}

export function runtime() {
  return { path: process.execPath, version: process.version.replace(/^v/, '') };
}
