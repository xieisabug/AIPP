import { readFileSync } from 'node:fs';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { jsonLines } from './common.mjs';
import { Type } from 'typebox';

// Pi's own read-only tools. AIPP tools use the bridge's per-call classification.
export const READ_ONLY_TOOLS = new Set(['read', 'grep', 'find', 'ls']);

// Runs inside Pi, using its extension/tool API. A failed MCP handshake must stop
// admission in the parent bridge instead of producing a session missing tools.
export default async function aippExtension(pi) {
  try {
    const contextFile = process.env.AIPP_PI_CONTEXT_FILE;
    const instructions = contextFile ? readFileSync(contextFile, 'utf8') : '';
    if (instructions) pi.on('before_agent_start', event => {
      const options = event.systemPromptOptions;
      if (options?.sections && options.forceSystemPrompt === undefined) {
        options.sections.aipp_butler = instructions;
      } else {
        return { systemPrompt: event.systemPrompt + '\n\n' + instructions };
      }
    });
    const approval = process.env.AIPP_PI_APPROVAL || 'all';
    const registrations = new Map();
    let reviewBridgeCall;
    if (!['all', 'read_only', 'none'].includes(approval)) throw new Error(`Unsupported Pi approval scope '${approval}'`);
    if (approval !== 'none') pi.on('tool_call', async (event, context) => {
      if (approval === 'read_only' && READ_ONLY_TOOLS.has(event.toolName)) return;
      if (registrations.has(event.toolName)) {
        const matches = pi.getAllTools?.().filter(tool => tool.name === event.toolName);
        const active = matches?.length === 1 && matches[0];
        const ownsTool = active && (active.sourceInfo?.path
          ? active.sourceInfo.path === fileURLToPath(import.meta.url)
          : active.parameters === registrations.get(event.toolName));
        if (ownsTool) {
          const review = await reviewBridgeCall(event.toolName, event.input).catch(() => null);
          if (review?.exempt === true) return;
        }
      }
      if (!(await context.ui.confirm(`允许 Pi 使用 ${event.toolName}`, JSON.stringify(event.input)))) {
        return { block: true, reason: 'User denied this tool call' };
      }
    });
    const config = process.env.AIPP_PI_MCP ? JSON.parse(process.env.AIPP_PI_MCP) : null;
    if (config) {
      const child = spawn(config.command, config.args, { env: { ...process.env, ...config.env }, windowsHide: true,
        stdio: ['pipe', 'pipe', 'pipe'] });
      let sequence = 0;
      const pending = new Map();
      let failed;
      const fail = error => {
        failed = error;
        for (const waiter of pending.values()) waiter.reject(error);
        pending.clear();
      };
      child.on('error', fail);
      child.on('exit', (code, signal) => fail(new Error(`AIPP MCP bridge exited (${code}, ${signal})`)));
      child.stderr.on('data', data => process.stderr.write(data));
      jsonLines(child.stdout, response => {
          const waiter = pending.get(response.id);
          if (!waiter) return;
          pending.delete(response.id);
          if (response.error) waiter.reject(new Error(JSON.stringify(response.error)));
          else waiter.resolve(response.result);
      }, fail);
      const call = (method, params, waitsForAnswer = false) => new Promise((resolve, reject) => {
        if (failed) return reject(failed);
        const id = ++sequence;
        const timer = waitsForAnswer ? undefined : setTimeout(() => { pending.delete(id); reject(new Error(`AIPP MCP ${method} timed out`)); }, method === 'tools/call' ? 600000 : 30000);
        pending.set(id, { resolve: value => { clearTimeout(timer); resolve(value); }, reject: error => { clearTimeout(timer); reject(error); } });
        child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', id, method, params })}\n`);
      });
      await call('initialize', { protocolVersion: '2024-11-05', capabilities: {}, clientInfo: { name: 'aipp-pi', version: '1' } });
      reviewBridgeCall = (name, args) => call('aipp/reviewExemption', { name, arguments: args });
      child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' })}\n`);
      let cursor;
      do {
        const result = await call('tools/list', cursor ? { cursor } : {});
        for (const tool of result.tools) {
          const parameters = Type.Unsafe({ ...tool.inputSchema, type: tool.inputSchema?.type || 'object', properties: tool.inputSchema?.properties || {} });
          registrations.set(tool.name, parameters);
          pi.registerTool({ name: tool.name, label: tool.name, description: tool.description || tool.name,
            parameters,
            async execute(_id, params) {
              const result = await call('tools/call', { name: tool.name, arguments: params }, tool._meta?.['aipp/waitsForAnswer'] === true);
              if (result.isError) throw new Error(JSON.stringify(result.content));
              return { content: result.content, details: {} };
            },
          });
        }
        cursor = result.nextCursor;
      } while (cursor);
      pi.on('session_shutdown', async () => { child.kill(); });
      pi.on('before_agent_start', async () => { if (failed) throw failed; });
      process.once('exit', () => child.kill());
    }
    process.stderr.write('AIPP_PI_EXTENSION_READY\n');
  } catch (error) {
    process.stderr.write(`AIPP_PI_EXTENSION_ERROR:${JSON.stringify(error.message)}\n`);
    throw error;
  }
}
