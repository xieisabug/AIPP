import test from 'node:test';
import assert from 'node:assert/strict';
import { PassThrough } from 'node:stream';
import { jsonLines, splitModel } from './common.mjs';
import { tick } from './test-helpers.mjs';

test('should reassemble split UTF-8 and several Pi records when native stdout chunks cross frame boundaries', async () => {
  const stream = new PassThrough(); const frames = [], errors = [];
  jsonLines(stream, value => frames.push(value), error => errors.push(error));
  const bytes = Buffer.from('{"text":"你好\\nworld"}\r\n{"done":true}\n');
  for (const byte of bytes) stream.write(Buffer.from([byte]));
  stream.end(); await tick();
  assert.deepEqual(frames, [{ text: '你好\nworld' }, { done: true }]);
  assert.deepEqual(errors, []);
});

test('should report oversized unfinished native records instead of silently dropping protocol data', () => {
  const stream = new PassThrough(); const errors = [];
  jsonLines(stream, () => assert.fail('unexpected parsed frame'), error => errors.push(error));
  stream.write('x'.repeat(16 * 1024 * 1024 + 1));
  assert.match(errors[0].message, /exceeds 16 MiB/);
  stream.destroy();
});

test('should preserve Unicode separators and reject malformed NDJSON when reading native frames', async () => {
  const stream = new PassThrough(); const frames = [], errors = [];
  jsonLines(stream, value => frames.push(value), error => errors.push(error));
  stream.write('{"text":"甲\u2028乙\u2029丙"}\r\n{broken}\n');
  await tick();
  assert.equal(frames[0].text, '甲\u2028乙\u2029丙'); assert.equal(errors.length, 1);
});

test('should preserve nested model IDs and reject missing provider when choosing native models', () => {
  assert.deepEqual(splitModel('provider/family/model'), { providerID: 'provider', modelID: 'family/model' });
  assert.throws(() => splitModel('model'), /provider\/model/);
  assert.throws(() => splitModel('provider/'), /provider\/model/);
});
