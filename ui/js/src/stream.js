// Decodifica el formato binario de gryphus_ipc::stream (ver docs/architecture/ipc.md).
// | 0 Scrollback | 1 Data | 2 Exited (i32 LE, i32::MIN = sin código) | 3 Failed (UTF-8) |

export const NO_CODE = -2147483648;

export function toBytes(message) {
  if (message instanceof ArrayBuffer) return new Uint8Array(message);
  if (ArrayBuffer.isView(message)) {
    return new Uint8Array(message.buffer, message.byteOffset, message.byteLength);
  }
  if (Array.isArray(message)) return Uint8Array.from(message);
  return new Uint8Array(0);
}

/** Devuelve { kind, bytes?, code?, reason? } o null si el mensaje es inválido. */
export function decode(message) {
  const bytes = toBytes(message);
  if (bytes.length === 0) return null;
  const payload = bytes.subarray(1);
  switch (bytes[0]) {
    case 0:
      return { kind: 'scrollback', bytes: payload };
    case 1:
      return { kind: 'data', bytes: payload };
    case 2: {
      if (payload.length !== 4) return null;
      const code = new DataView(payload.buffer, payload.byteOffset, 4).getInt32(0, true);
      return { kind: 'exited', code: code === NO_CODE ? null : code };
    }
    case 3:
      return { kind: 'failed', reason: new TextDecoder().decode(payload) };
    default:
      return null;
  }
}
