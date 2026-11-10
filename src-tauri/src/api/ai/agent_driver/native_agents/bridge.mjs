import { jsonLines } from './common.mjs';
import { openPi } from './pi.mjs';
import { openCursor } from './cursor.mjs';
import { openOpenCode } from './opencode.mjs';
import { openAcpNative } from './acp-native.mjs';

const drivers = { pi: openPi, cursor: openCursor, opencode: openOpenCode,
  kiro: (config, emit) => openAcpNative('kiro', config, emit),
  antigravity: (config, emit) => openAcpNative('antigravity', config, emit) };

const emit = event => process.stdout.write(`${JSON.stringify(event)}\n`);
console.log = (...args) => console.error(...args);
let runtime, initializing = false, fatal = false;
const fail = error => { if (!fatal) { fatal = true; emit({ type: 'fatal', error: error.message }); } };
jsonLines(process.stdin, request => {
  void (async () => {
    if (fatal) throw new Error('Native agent bridge has failed');
    let data;
    if (request.type === 'login') {
      if (request.driver !== 'antigravity') throw new Error('Browser login is only supported by Antigravity');
      const login = await drivers.antigravity({ ...request.config, login: true }, emit);
      await login.close();
      data = { authenticated: true };
    } else if (request.type === 'probe') {
      const open = drivers[request.driver];
      if (!open) throw new Error(`Unknown native agent driver '${request.driver}'`);
      const probe = await open({ ...request.config, probe: true }, emit);
      try { data = await probe.models(); } finally { await probe.close(); }
    } else if (request.type === 'init') {
      if (runtime || initializing) throw new Error('Native agent bridge already initialized');
      initializing = true;
      const open = drivers[request.driver];
      if (!open) throw new Error(`Unknown native agent driver '${request.driver}'`);
      runtime = await open(request.config, emit);
      data = { session: runtime.session, config_options: runtime.configOptions };
    } else {
      if (!runtime) throw new Error('Native agent bridge is not initialized');
      switch (request.type) {
        case 'prompt': await runtime.prompt(request.text, request.turn); break;
        case 'cancel': await runtime.cancel(); break;
        case 'config': await runtime.configure(request.key, request.value); break;
        case 'reply': await runtime.reply(request.request_id, request.value); break;
        case 'answer': await runtime.answer(request.request_id, request.answers); break;
        case 'models': data = await runtime.models(); break;
        case 'close': await runtime.close(); process.exit(0); break;
        default: throw new Error(`Unknown bridge request '${request.type}'`);
      }
    }
    emit({ type: 'response', id: request.id, ok: true, data });
  })().catch(error => emit({ type: 'response', id: request.id, ok: false, error: error.message }));
}, fail);
process.stdin.on('end', () => { void runtime?.close().finally(() => process.exit(0)); });
process.on('uncaughtException', fail);
process.on('unhandledRejection', fail);
