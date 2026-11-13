import { spawn } from 'node:child_process';
import { StringDecoder } from 'node:string_decoder';
import { existsSync, readFileSync } from 'node:fs';
import { delimiter, dirname, extname, join, isAbsolute } from 'node:path';

export function splitModel(model) {
  const index = model.indexOf('/');
  if (index < 1 || index === model.length - 1) throw new Error(`Model '${model}' must use provider/model format`);
  return { providerID: model.slice(0, index), modelID: model.slice(index + 1) };
}

// The file a command name resolves to, using the same search order as launch() on Windows
// (PATH x .exe/.ps1/.cmd/bare) and a plain PATH scan elsewhere. null when nothing matches.
export function resolveCommand(command, platform = process.platform, path = process.env.PATH || '', exists = existsSync) {
  const separator = platform === process.platform ? delimiter : platform === 'win32' ? ';' : ':';
  if (platform === 'win32') {
    const roots = isAbsolute(command) ? [''] : path.split(separator);
    const extensions = extname(command) ? [''] : ['.exe', '.ps1', '.cmd', ''];
    return roots.flatMap(root => extensions.map(ext => root ? join(root, command + ext) : command + ext)).find(exists) ?? null;
  }
  if (command.includes('/')) return exists(command) ? command : null;
  return path.split(separator).filter(Boolean).map(root => join(root, command)).find(exists) ?? null;
}

// What launch() will spawn, without spawning: { file, args, resolved }.
export function launchPlan(command, args, platform = process.platform) {
  if (platform === 'win32') {
    command = resolveCommand(command, platform) ?? command;
    if (/\.ps1$/i.test(command)) {
      const shim = readFileSync(command, 'utf8');
      const relative = shim.match(/"\$basedir\/([^"\r\n]+)"\s+\$args/)?.[1]
        || shim.match(/\$launcher=Join-Path \$basedir "([^"\r\n]+)"/)?.[1];
      if (!relative) throw new Error(`Cannot resolve Node entry point from ${command}; configure the CLI executable or JavaScript entry path`);
      const entry = join(dirname(command), relative);
      if (!existsSync(entry)) throw new Error(`CLI entry point does not exist: ${entry}`);
      return { file: process.execPath, args: [entry, ...args], resolved: command };
    }
    // Invoke Windows npm shims without concatenating caller-controlled arguments into a shell command.
    if (/\.(cmd|bat)$/i.test(command)) {
      throw new Error(`Use the executable or JavaScript CLI entry point instead of a shell shim: ${command}`);
    }
  }
  const script = /\.[cm]?js$/i.test(command);
  return { file: script ? process.execPath : command, args: script ? [command, ...args] : args, resolved: command };
}

export function launch(command, args, options = {}) {
  const plan = launchPlan(command, args);
  return spawn(plan.file, plan.args, { ...options, windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'] });
}

export function jsonLines(stream, receive, fail) {
  // Only LF delimits protocol frames. Node readline also splits U+2028/U+2029.
  const decoder = new StringDecoder('utf8');
  let buffer = '';
  const parse = line => {
    if (!line.trim()) return;
    try { receive(JSON.parse(line)); } catch (error) { fail(error); }
  };
  stream.on('data', chunk => {
    buffer += decoder.write(chunk);
    let end;
    while ((end = buffer.indexOf('\n')) >= 0) {
      const line = buffer.slice(0, end).replace(/\r$/, ''); buffer = buffer.slice(end + 1); parse(line);
    }
    if (buffer.length > 16 * 1024 * 1024) { buffer = ''; fail(new Error('Native JSON frame exceeds 16 MiB')); }
  });
  stream.on('end', () => { buffer += decoder.end(); if (buffer) parse(buffer); });
  stream.on('error', fail);
}

export function deadline(promise, label, milliseconds = 30000) {
  let timer;
  return Promise.race([promise, new Promise((_, reject) => {
    timer = setTimeout(() => reject(new Error(`${label} timed out after ${milliseconds}ms`)), milliseconds);
  })]).finally(() => clearTimeout(timer));
}

export async function stopChild(child) {
  if (process.platform === 'win32' && child.pid) {
    await deadline(new Promise(resolve => {
      const killer = spawn('taskkill.exe', ['/PID', String(child.pid), '/T', '/F'], { windowsHide: true, stdio: 'ignore' });
      killer.once('error', resolve); killer.once('exit', resolve);
    }), 'Native process tree shutdown', 5000).catch(() => {});
  }
  child.kill();
}

// Explicit phase boundaries keep later reasoning out of an earlier thought block.
export function thinkingStream(emit) {
  let turn, id, count = 0;
  return {
    delta(current, text) {
      if (turn !== current) { turn = current; count = 0; id = undefined; }
      id ||= `thinking:${++count}`;
      emit({ type: 'activity', turn, id, kind: 'reasoning', status: 'running', append: true, output: text });
    },
    end(current, status = 'success') {
      if (!id || current !== turn) return;
      emit({ type: 'activity', turn, id, kind: 'reasoning', status });
      id = undefined;
    },
  };
}

export function readableResult(value) {
  if (value == null) return undefined;
  if (typeof value === 'string') return value;
  if (Array.isArray(value)) return value.map(readableResult).filter(Boolean).join('\n');
  if (typeof value.text === 'string') return value.text;
  if (typeof value.text?.text === 'string') return value.text.text;
  if (value.image) return '[图片结果]';
  if (Array.isArray(value.content)) return value.content.map(part => part.type === 'image' ? '[图片结果]' : readableResult(part)).filter(Boolean).join('\n');
  if (typeof value.stdout === 'string' || typeof value.stderr === 'string') return [value.stdout, value.stderr].filter(Boolean).join('\n');
  for (const key of ['output', 'fileContent', 'fileText', 'content', 'message']) if (typeof value[key] === 'string') return value[key];
  if (Array.isArray(value.files)) return value.files.map(file => typeof file === 'string' ? file : file.path || JSON.stringify(file)).join('\n');
  return JSON.stringify(value);
}
