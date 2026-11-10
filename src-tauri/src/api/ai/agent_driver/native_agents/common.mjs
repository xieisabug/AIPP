import { spawn } from 'node:child_process';
import { StringDecoder } from 'node:string_decoder';
import { existsSync, readFileSync } from 'node:fs';
import { delimiter, dirname, extname, join, isAbsolute } from 'node:path';

export function splitModel(model) {
  const index = model.indexOf('/');
  if (index < 1 || index === model.length - 1) throw new Error(`Model '${model}' must use provider/model format`);
  return { providerID: model.slice(0, index), modelID: model.slice(index + 1) };
}

export function launch(command, args, options = {}) {
  if (process.platform === 'win32') {
    const roots = isAbsolute(command) ? [''] : (process.env.PATH || '').split(delimiter);
    const extensions = extname(command) ? [''] : ['.exe', '.ps1', '.cmd', ''];
    const resolved = roots.flatMap(root => extensions.map(ext => root ? join(root, command + ext) : command + ext)).find(existsSync);
    if (resolved) command = resolved;
    if (/\.ps1$/i.test(command)) {
      const shim = readFileSync(command, 'utf8');
      const relative = shim.match(/"\$basedir\/([^"\r\n]+)"\s+\$args/)?.[1]
        || shim.match(/\$launcher=Join-Path \$basedir "([^"\r\n]+)"/)?.[1];
      if (!relative) throw new Error(`Cannot resolve Node entry point from ${command}; configure the CLI executable or JavaScript entry path`);
      const entry = join(dirname(command), relative);
      if (!existsSync(entry)) throw new Error(`CLI entry point does not exist: ${entry}`);
      return spawn(process.execPath, [entry, ...args], { ...options, windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'] });
    }
  }
  // Invoke Windows npm shims without concatenating caller-controlled arguments into a shell command.
  if (process.platform === 'win32' && /\.(cmd|bat)$/i.test(command)) {
    throw new Error(`Use the executable or JavaScript CLI entry point instead of a shell shim: ${command}`);
  }
  return spawn(/\.[cm]?js$/i.test(command) ? process.execPath : command,
    /\.[cm]?js$/i.test(command) ? [command, ...args] : args,
    { ...options, windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'] });
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
