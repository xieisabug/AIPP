import { EventEmitter } from 'node:events';
import { PassThrough, Writable } from 'node:stream';
export const tick = () => new Promise(resolve => setImmediate(resolve));
export function processFixture(receive = () => {}) {
  const child = new EventEmitter();
  child.stdout = new PassThrough(); child.stderr = new PassThrough();
  child.stdin = new Writable({ write(chunk, _encoding, next) { receive(JSON.parse(chunk.toString())); next(); } });
  child.kill = () => { child.killed = true; child.emit('exit', null, 'SIGTERM'); };
  return child;
}
export function piFixture({ rejectRestore = false, state = { sessionFile: '/native/session.jsonl' }, receipt = {} } = {}) {
  const commands = [];
  const child = processFixture(command => {
    commands.push(command);
    if (command.type === 'extension_ui_response') return;
    child.stdout.write(`${JSON.stringify({ type: 'response', id: command.id, command: command.type,
      success: !(rejectRestore && command.type === 'switch_session'), error: 'session file missing',
      data: command.type === 'get_state' ? state : command.type === 'prompt' ? receipt : {} })}\n`);
  });
  const emit = frame => child.stdout.write(`${JSON.stringify(frame)}\n`);
  const launches = [];
  const launch = (...args) => { launches.push(args); queueMicrotask(() => child.stderr.write('AIPP_PI_EXTENSION_READY\n')); return child; };
  return { child, commands, emit, launch, launches };
}

export function openCodeFixture({ respond } = {}) {
  const requests = [], events = [];
  let controller;
  const stream = new ReadableStream({ start(value) { controller = value; } });
  const child = processFixture();
  const launches = [];
  const launch = (...args) => { launches.push(args); queueMicrotask(() => child.stdout.write('opencode server listening on http://127.0.0.1:12345\n')); return child; };
  const fetch = async (url, options) => {
    const path = new URL(url).pathname;
    requests.push({ path, ...options, body: options.body ? JSON.parse(options.body) : undefined });
    const override = await respond?.(path, options);
    if (override) return override;
    if (path === '/event') return new Response(stream);
    if (path === '/session' || path === '/session/s1') return Response.json({ id: 's1' });
    return Response.json({});
  };
  const event = (type, properties) => controller.enqueue(new TextEncoder().encode(`data: ${JSON.stringify({ type, properties })}\n\n`));
  return { child, launch, launches, fetch, requests, events, event, end: () => controller.close(), raw: value => controller.enqueue(new TextEncoder().encode(value)) };
}
