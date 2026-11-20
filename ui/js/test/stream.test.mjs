// El decodificador JS debe coincidir con gryphus_ipc::stream (Rust).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { decode, NO_CODE } from '../src/stream.js';

const i32le = (n) => {
  const b = new Uint8Array(4);
  new DataView(b.buffer).setInt32(0, n, true);
  return [...b];
};

test('scrollback y data conservan los bytes', () => {
  assert.deepEqual([...decode(new Uint8Array([0, 104, 105]).buffer).bytes], [104, 105]);
  assert.equal(decode([1, 27]).kind, 'data');
});

test('exited con y sin código', () => {
  assert.deepEqual(decode([2, ...i32le(3)]), { kind: 'exited', code: 3 });
  assert.deepEqual(decode([2, ...i32le(-1)]), { kind: 'exited', code: -1 });
  assert.deepEqual(decode([2, ...i32le(NO_CODE)]), { kind: 'exited', code: null });
});

test('failed decodifica UTF-8', () => {
  const reason = [...new TextEncoder().encode('no encontrado ñ')];
  assert.deepEqual(decode([3, ...reason]), { kind: 'failed', reason: 'no encontrado ñ' });
});

test('mensajes inválidos', () => {
  assert.equal(decode([]), null);
  assert.equal(decode([9]), null);
  assert.equal(decode([2, 1, 2]), null);
});
