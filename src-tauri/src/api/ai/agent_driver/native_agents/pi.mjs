import { RequestRejected, deadline, jsonLines, launch, permissionOption, splitModel, stopChild } from './common.mjs';
import { writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

// all: every tool call; read_only: read/grep/find/ls run without asking; none: Pi's native behaviour.
export const PI_APPROVAL_SCOPES = ['all', 'read_only', 'none'];

// Pi's execution events cover both built-in operations and extension/MCP tools.
// Keep the native result for inspection, but expose readable content to the UI.
function resultText(result) {
  if (result == null) return undefined;
  if (typeof result === 'string') return result;
  if (Array.isArray(result.content)) return result.content.map(part =>
    part.type === 'text' ? part.text : part.type === 'image' ? '[图片结果]' : `[${part.type || '非文本结果'}]`).join('\n');
  return JSON.stringify(result);
}

function toolActivity(frame, previous, cwd) {
  const tool = frame.toolName || previous?.metadata.tool;
  const input = frame.args ?? previous?.input;
  const result = frame.result ?? frame.partialResult;
  const metadata = { ...previous?.metadata, tool, cwd };
  if (result !== undefined) metadata.rawResult = result;
  const path = typeof input?.path === 'string' ? input.path : '';
  let kind = 'tool', title = tool;
  if (['bash', 'powershell'].includes(tool)) {
    kind = 'command'; title = input?.command || tool;
    metadata.command = title;
  } else if (tool === 'read') {
    kind = 'file_read'; title = `读取 ${path}`.trim();
  } else if (['grep', 'find', 'ls'].includes(tool)) {
    kind = 'file_search';
    title = [tool === 'ls' ? '列出目录' : '搜索', input?.pattern, path].filter(Boolean).join(' ');
  } else if (tool === 'edit') {
    kind = 'patch'; title = `编辑 ${path}`.trim();
    metadata.changes = [{ path, kind: 'update', diff: result?.details?.patch || result?.details?.diff }];
  } else if (tool === 'write') {
    kind = 'file_write'; title = `写入 ${path}`.trim();
  }
  const output = resultText(result);
  return { type: 'activity', id: frame.toolCallId, kind, title, input, metadata,
    status: frame.type === 'tool_execution_end' ? (frame.isError ? 'failed' : 'success') : 'running',
    ...(output !== undefined ? { output } : {}),
    ...(frame.isError ? { error: output || 'Pi 工具执行失败，但原生结果未提供错误详情' } : {}) };
}

export async function openPi(config, emit, dependencies = {}) {
  const approval = config.pi_approval || 'all';
  if (!PI_APPROVAL_SCOPES.includes(approval)) throw new Error(`Unsupported Pi approval scope '${approval}'; expected ${PI_APPROVAL_SCOPES.join(', ')}`);
  // The actor owns this private directory until the Pi process has stopped.
  // Pass a path so long role/skill instructions do not exceed Windows env limits.
  const contextFile = config.butlerInstructions ? join(config.temp_root, 'pi-butler.txt') : '';
  if (contextFile) await writeFile(contextFile, config.butlerInstructions, { mode: 0o600, flag: 'wx' });
  const child = (dependencies.launch || launch)(config.command || 'pi', ['--mode', 'rpc', '--extension', fileURLToPath(new URL('./pi-extension.mjs', import.meta.url))],
    { cwd: config.cwd, env: { ...process.env, AIPP_PI_APPROVAL: approval, AIPP_PI_CONTEXT_FILE: contextFile, ...(config.mcp ? { AIPP_PI_MCP: JSON.stringify(config.mcp) } : {}) } });
  let sequence = 0, turn = null, failure, session, closing = false;
  let awaitingSettlement = false, modelError, finalText;
  const pending = new Map();
  const questions = new Map();
  const tools = new Map();
  let thinkingId, thinkingSequence = 0;
  const send = data => child.stdin.write(`${JSON.stringify(data)}\n`);
  let previousTurn, wakes = 0, cancelling = false;
  const startTurn = id => {
    turn = id; cancelling = false; failure = undefined; modelError = undefined; finalText = undefined;
    tools.clear(); thinkingId = undefined; thinkingSequence = 0;
  };
  const finishTurn = (aborted = false) => {
    const cancelled = cancelling || aborted;
    previousTurn = !cancelled && !failure && !modelError ? turn : undefined;
    emit({ type: 'done', turn, cancelled, error: failure || (cancelled ? undefined : modelError), final_text: finalText });
    turn = null; awaitingSettlement = false; questions.clear(); tools.clear(); thinkingId = undefined;
  };
  const fail = error => {
    for (const { reject } of pending.values()) reject(error);
    pending.clear();
    if (!closing) emit({ type: 'fatal', error: error.message });
  };
  child.on('error', fail);
  const extensionReady = new Promise((resolve, reject) => {
    let buffer = '';
    child.stderr.on('data', data => {
      process.stderr.write(data);
      buffer = (buffer + data).slice(-16384);
      if (buffer.includes('AIPP_PI_EXTENSION_READY\n')) resolve();
      const failure = buffer.match(/AIPP_PI_EXTENSION_ERROR:(.*)\n/);
      if (failure) reject(new Error(`Pi extension initialization failed: ${failure[1]}`));
    });
    child.once('error', reject);
    child.once('exit', code => reject(new Error(`Pi exited before extension initialization (${code})`)));
  });
  child.on('exit', (code, signal) => fail(new Error(`Pi exited (code=${code}, signal=${signal})`)));
  const request = data => deadline(new Promise((resolve, reject) => {
    const id = String(++sequence);
    pending.set(id, { resolve, reject });
    send({ ...data, id });
  }), `Pi ${data.type}`);
  jsonLines(child.stdout, frame => {
    if (frame.type === 'extension_error') {
      fail(new Error(`Pi extension ${frame.extensionPath || '<unknown>'} (${frame.event || 'unknown event'}): ${frame.error || 'extension_error omitted its cause'}`));
      return;
    }
    if (frame.type === 'response') {
      const waiter = pending.get(frame.id);
      if (!waiter) return;
      pending.delete(frame.id);
      if (frame.success === false && typeof frame.error === 'string') waiter.reject(new RequestRejected(`Pi ${frame.command}: ${frame.error}`));
      else if (frame.success === true) waiter.resolve(frame.data);
      else waiter.reject(new Error(`Pi ${frame.command} returned an invalid response for request ${frame.id}`));
      return;
    }
    if (!turn && previousTurn && !cancelling && !closing && frame.type === 'agent_start') {
      startTurn(`wake:${++wakes}`);
      emit({ type: 'wake', session, after: previousTurn, turn });
    }
    if (!turn) return;
    const base = { turn };
    if (frame.type === 'message_update') {
      const event = frame.assistantMessageEvent;
      if (event?.type === 'text_delta') emit({ ...base, type: 'text', text: event.delta });
      if (event?.type === 'thinking_delta') {
        thinkingId ||= `thinking:${++thinkingSequence}`;
        emit({ ...base, type: 'activity', id: thinkingId, kind: 'reasoning', status: 'running', output: event.delta, append: true });
      }
      if (event?.type === 'thinking_end' && thinkingId) {
        emit({ ...base, type: 'activity', id: thinkingId, kind: 'reasoning', status: 'success' });
        thinkingId = undefined;
      }
    }
    if (['tool_execution_start', 'tool_execution_update', 'tool_execution_end'].includes(frame.type)) {
      if (!frame.toolCallId) { fail(new Error('Pi tool execution event omitted toolCallId')); return; }
      const activity = toolActivity(frame, tools.get(frame.toolCallId), config.cwd);
      tools.set(frame.toolCallId, activity);
      emit({ ...base, ...activity });
    }
    if (frame.type === 'message_end' && frame.message?.role === 'assistant') {
      // agent_end repeats the individual message_end records.
      const usage = frame.message.usage;
      if (usage) emit({ type: 'usage', turn, usage: { inputTokens: usage.input, outputTokens: usage.output,
        cachedReadTokens: usage.cacheRead, cachedWriteTokens: usage.cacheWrite } });
      modelError = frame.message.stopReason === 'error' ? frame.message.errorMessage || 'Pi model error has no errorMessage' : undefined;
    }
    if (frame.type === 'auto_retry_end' && !frame.success && frame.finalError) modelError = frame.finalError;
    if (frame.type === 'extension_ui_request' && frame.method === 'confirm') {
      emit({ type: 'permission', turn, id: frame.id, title: frame.title, input: frame.message,
        options: [{ id: 'allow', name: '本次允许', kind: 'allow_once' }, { id: 'deny', name: '拒绝', kind: 'reject_once' }] });
    } else if (frame.type === 'extension_ui_request' && ['input', 'select', 'editor'].includes(frame.method)) {
      const question = frame.title || frame.method;
      questions.set(frame.id, question);
      emit({ type: 'question', turn, id: frame.id, questions: [{ question, header: 'answer',
        options: (frame.options || []).map(label => ({ label, description: label })), multiSelect: false }] });
    } else if (frame.type === 'extension_ui_request' && !['notify', 'setStatus', 'setWidget', 'setTitle', 'setEditorText'].includes(frame.method)) {
      send({ type: 'extension_ui_response', id: frame.id, cancelled: true });
      failure = `Unsupported Pi extension UI method '${frame.method}'`;
      void request({ type: 'abort' }).catch(fail);
    }
    if (frame.type === 'agent_end') {
      const last = Array.isArray(frame.messages) ? frame.messages.findLast(message => message.role === 'assistant') : undefined;
      if (last?.stopReason === 'error') modelError = last.errorMessage || modelError || 'Pi model error has no errorMessage';
      else if (last) modelError = undefined;
      finalText = undefined;
      if (last?.stopReason === 'toolUse') finalText = '';
      else if (typeof last?.content === 'string') finalText = last.content;
      else if (Array.isArray(last?.content)) finalText = last.content.filter(block => block.type === 'text').map(block => block.text).join('');
      // Retry-aware Pi closes each attempt with agent_end, then the whole prompt with agent_settled.
      if (typeof frame.willRetry === 'boolean') awaitingSettlement = true;
      else finishTurn(last?.stopReason === 'aborted');
    } else if (frame.type === 'agent_settled' && awaitingSettlement) {
      finishTurn(frame.aborted === true);
    }
  }, fail);
  try {
    await deadline(extensionReady, 'Pi extension initialization');
    if (config.session) {
      const result = await request({ type: 'switch_session', sessionPath: config.session });
      if (result?.cancelled) throw new Error('Pi extension cancelled session restoration');
    }
    const state = await request({ type: 'get_state' });
    session = state.sessionFile;
    if (!session) throw new Error('Pi get_state did not return a persisted sessionFile');
    if (config.session && session !== config.session) throw new Error('Pi restored a different sessionFile');
    if (config.model) {
      const model = splitModel(config.model);
      await request({ type: 'set_model', provider: model.providerID, modelId: model.modelID });
    }
    return {
      session,
      configOptions: [permissionOption('pi_approval', '工具审批范围', approval, [
        ['all', '全部审批（all）'], ['read_only', '只读免审（read_only）'], ['none', '全部免审（none）'],
      ])],
      async prompt(text, id, images = []) {
        if (turn) throw new Error('Pi already has an active turn');
        startTurn(id);
        const receipt = await request({ type: 'prompt', message: text, ...(images.length ? { images: images.map(image => ({ type: 'image', ...image })) } : {}) });
        if (turn === id && receipt?.disposition === 'handled') {
          const state = await request({ type: 'get_state' });
          if (turn === id && !awaitingSettlement && state.isStreaming === false) finishTurn();
        }
      },
      async steer(expectedTurn, text, images = []) {
        if (!turn || turn !== expectedTurn) throw new Error('Pi steering target is no longer the active turn');
        await request({ type: 'steer', message: text, ...(images.length ? { images: images.map(image => ({ type: 'image', ...image })) } : {}) });
      },
      async cancel() { previousTurn = undefined; cancelling = true; await request({ type: 'abort' }); },
      async reply(id, value) { send({ type: 'extension_ui_response', id, confirmed: value === 'allow' }); },
      async answer(id, answers) {
        const question = questions.get(id);
        if (!question) throw new Error('Unknown or expired Pi question');
        if (answers && typeof answers[question] !== 'string') throw new Error('Pi question answer is missing');
        questions.delete(id);
        send({ type: 'extension_ui_response', id, ...(answers ? { value: answers[question] } : { cancelled: true }) });
      },
      async configure(key, value) {
        if (turn) throw new RequestRejected('Cannot change Pi configuration during a turn');
        if (key === 'model') {
          const model = splitModel(value);
          await request({ type: 'set_model', provider: model.providerID, modelId: model.modelID });
        } else if (key === 'thought_level') await request({ type: 'set_thinking_level', level: value });
        else throw new RequestRejected(`Unsupported Pi configuration: ${key}`);
      },
      async models() {
        const data = await request({ type: 'get_available_models' });
        return data.models.map(model => ({ id: `${model.provider}/${model.id}`, name: model.name || model.id }));
      },
      async close() { closing = true; await stopChild(child); },
    };
  } catch (error) { closing = true; await stopChild(child); throw error; }
}
