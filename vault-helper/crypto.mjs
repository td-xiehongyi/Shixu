import { argon2id } from 'hash-wasm';
import { createRequire } from 'node:module';
import { DOMParser, XMLSerializer } from '@xmldom/xmldom';
const require = createRequire(import.meta.url);
export const kdbx = require('./kdbx-lib/index.js');
// 0.9 parser no longer supports the old errorHandler object. No XML recovery/DTD.
globalThis.DOMParser = class {
  parseFromString(xml, type) {
    if (/<!DOCTYPE|<!ENTITY/i.test(xml)) throw Error('UNSUPPORTED');
    const document = new DOMParser({onError() { throw Error('AUTH_FAILED'); }}).parseFromString(xml, type);
    // Reject every XML binary path before KdbxWeb maps objects or decodes binary
    // values. Outer KDBX compression=0 does not exclude compressed Meta/Entry data.
    for (const element of document.getElementsByTagName('*')) {
      if (element.localName === 'Binary' || element.hasAttribute('Compressed')) throw Error('UNSUPPORTED');
    }
    return document;
  }
};
globalThis.XMLSerializer = XMLSerializer;
export async function argon2Adapter(password, salt, memory, iterations, length, parallelism, type, version) {
  if (type !== 2 || version !== 19 || length !== 32 || parallelism !== 1 ||
      !Number.isInteger(memory) || memory < 8 || memory > 65536 ||
      !Number.isInteger(iterations) || iterations < 1 || iterations > 4 ||
      !(password instanceof ArrayBuffer) || !(salt instanceof ArrayBuffer) || salt.byteLength < 8 || salt.byteLength > 32) throw Error('UNSUPPORTED');
  const result = await argon2id({password: new Uint8Array(password), salt: new Uint8Array(salt), memorySize: memory, iterations, parallelism, hashLength: length, outputType: 'binary'});
  const out = result.buffer.slice(result.byteOffset,result.byteOffset+result.byteLength);
  result.fill(0);
  return out;
}
kdbx.CryptoEngine.setArgon2Impl(argon2Adapter);
export const MAX_FILE = 8 * 1024 * 1024;
export function preflight(buffer) {
  if (buffer.length < 16 || buffer.length > MAX_FILE) throw Error('AUTH_FAILED');
  // Bound header and every TLV BEFORE library parsing/KDF allocation.
  let offset = 12;
  while (true) {
    if (offset + 5 > Math.min(buffer.length, 65536)) throw Error('AUTH_FAILED');
    const tag = buffer[offset], len = buffer.readUInt32LE(offset + 1); offset += 5;
    if (len > 65536 || offset + len > Math.min(buffer.length,65536)) throw Error('AUTH_FAILED');
    offset += len;
    if (tag === 0) break;
  }
  const bytes = buffer.buffer.slice(buffer.byteOffset,buffer.byteOffset+buffer.byteLength);
  const header = kdbx.KdbxHeader.read(new kdbx.BinaryStream(bytes), new kdbx.KdbxContext({ kdbx: {} }));
  // Initial Shixu format is uncompressed. Reject gzip BEFORE any expansion.
  if (header.versionMajor !== 4 || header.compression !== 0 || header.dataCipherUuid.toString() !== kdbx.Consts.CipherId.Aes) throw Error('UNSUPPORTED');
  const params = header.kdfParameters;
  // Type and duplicate-key inspection is tied to the pinned upstream source.
  const types = {'$UUID':66,S:66,P:4,I:5,M:5,V:4};
  if(params.length!==6 || new Set(params.keys()).size!==6 || params._items.some(item=>types[item.key]!==item.type)) throw Error('UNSUPPORTED');
  const num = key => { const v = params.get(key); return v instanceof kdbx.Int64 ? v.value : v; };
  const salt = params.get('S'), uuid = params.get('$UUID');
  if (!(uuid instanceof ArrayBuffer) || Buffer.from(uuid).toString('base64') !== kdbx.Consts.KdfId.Argon2id ||
      num('V') !== 19 || num('P') !== 1 || !Number.isSafeInteger(num('M')) || num('M') < 8192 || num('M') > 67108864 || num('M') % 1024 ||
      !Number.isSafeInteger(num('I')) || num('I') < 1 || num('I') > 4 ||
      !(salt instanceof ArrayBuffer) || salt.byteLength !== 32 || params.get('K') !== undefined || params.get('A') !== undefined) throw Error('UNSUPPORTED');
  return bytes;
}
export function validateText(value, noNewline = false) {
  if (typeof value !== 'string' || value.length === 0 || Buffer.byteLength(value) > 65536 ||
      /[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/u.test(value) ||
      (noNewline && /[\r\n\u0085\u2028\u2029]/u.test(value))) throw Error('INVALID_INPUT');
  return value;
}
