import { openAcpNative } from './acp-native.mjs';
import { processFixture } from './test-helpers.mjs';

export const select = (id, values, currentValue = values[0]) => ({ id, name: id, category: id, type: 'select', currentValue, options: values.map(value => ({ value, name: value })) });
export function acpFixture({ respond, options = [select('model', ['auto', 'model-a']), select('mode', ['default', 'review']), select('autopilot', ['on', 'off']), select('effortLevel', ['low', 'high'])], capabilities = { loadSession: true, sessionCapabilities: { resume: {} } } } = {}) {
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
    finish: (stopReason = 'end_turn', usage) => write({ id: prompt.id, result: { stopReason, usage } }),
    open: (driver, config = {}, dependencies = {}) => openAcpNative(driver, { cwd: process.cwd(), ...config }, event => events.push(event), {
      launch: (...args) => { launches.push(args); return child; },
      antigravityLaunch: async () => ({ command: 'agy', args: ['acp'], env: {}, cleanup: async () => { cleaned = true; } }),
      ...dependencies,
    }),
  };
}
