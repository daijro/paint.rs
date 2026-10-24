// Task 48w fix 1: an absolute AV1 bit position must not wrap at a 256 MiB item offset.
// Run: node tests/web-codecs-av1-offset.mjs [Trunk dist directory]
// The large fixture stays in this process. Neither stream reaches a real decoder, even on red.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
let moduleFile = path.join(repo, 'crates/paintrs/src/platform/web_shim.js');
if (process.argv[2]) {
  const dist = path.resolve(process.argv[2]);
  const modules = fs.readdirSync(dist, { recursive: true }).filter(f => f.endsWith('/web_shim.js'));
  assert.equal(modules.length, 1, 'test the single actual Trunk-generated shim');
  moduleFile = path.join(dist, modules[0]);
}
const source = fs.readFileSync(moduleFile, 'utf8');
const { decodeImage } = await import(`data:text/javascript;base64,${Buffer.from(source).toString('base64')}`);

// Repack a.avif's reduced still-picture sequence to claim 32769 × 16, keeping ispe 13 × 9.
// The fixture and padding OBU construction also appear in the independent review probe.
const original = fs.readFileSync(path.join(repo, 'tests/fixtures/a.avif'));
const mdat = original.indexOf('mdat');
const start = mdat + 6; // after temporal delimiter 12 00
assert.equal(original[start], 0x0a);
const oldLength = original[start + 1];
const payload = original.subarray(start + 2, start + 2 + oldLength);
const bits = Array.from(payload).flatMap(v => Array.from({ length: 8 }, (_, i) => v >> (7 - i) & 1));
assert.equal(bits[4], 1, 'fixture has a reduced still-picture sequence');
const field = (at, n) => bits.slice(at, at + n).reduce((v, b) => v * 2 + b, 0);
const oldEnd = 18 + field(10, 4) + 1 + field(14, 4) + 1;
const replacement = [[15, 4], [15, 4], [32768, 16], [15, 16]]
  .flatMap(([v, n]) => Array.from({ length: n }, (_, i) => v >> (n - 1 - i) & 1));
const repacked = [...bits.slice(0, 10), ...replacement, ...bits.slice(oldEnd)];
assert.equal(repacked.length % 8, 0);
const sequence = Buffer.from(Array.from({ length: repacked.length / 8 }, (_, i) =>
  repacked.slice(i * 8, i * 8 + 8).reduce((v, b) => v * 2 + b, 0)));
const small = Buffer.concat([original.subarray(0, start + 1), Buffer.from([sequence.length]), sequence, original.subarray(start + 2 + oldLength)]);
const delta = sequence.length - oldLength;
const extentSize = Buffer.alloc(4);
extentSize.writeUInt32BE(original.length - mdat - 4);
const extentAt = small.indexOf(extentSize, small.indexOf('iloc'));
assert.ok(extentAt > 0 && extentAt < small.indexOf('iinf'));
small.writeUInt32BE(original.readUInt32BE(mdat - 4) + delta, mdat - 4);
small.writeUInt32BE(extentSize.readUInt32BE() + delta, extentAt);

const paddingBytes = 2 ** 28;
const large = Buffer.alloc(small.length + paddingBytes);
small.copy(large, 0, 0, mdat + 4);
let remaining = paddingBytes - 5;
large[mdat + 4] = 0x7a; // legal padding OBU, four-byte LEB128 payload length
for (let i = 0; i < 4; i++) {
  large[mdat + 5 + i] = remaining % 128 | (i < 3 ? 128 : 0);
  remaining = Math.floor(remaining / 128);
}
assert.equal(remaining, 0);
small.copy(large, mdat + 4 + paddingBytes, mdat + 4);
large.writeUInt32BE(small.readUInt32BE(mdat - 4) + paddingBytes, mdat - 4);
large.writeUInt32BE(small.readUInt32BE(extentAt) + paddingBytes, extentAt);
assert.ok(large.length < 536870912, 'large offset remains inside the admitted encoded-byte budget');

let bitmapCalls = 0;
let fallbackCalls = 0;
globalThis.Blob = class {}; // no extra 256 MiB copy if admission regresses
// A deliberately harmless success makes bypass observable without decoding hostile bytes.
globalThis.createImageBitmap = async () => { bitmapCalls++; return { width: 1, height: 1, close() {} }; };
globalThis.document = {
  baseURI: 'http://127.0.0.1/',
  createElement(kind) {
    if (kind !== 'canvas') { fallbackCalls++; throw new Error('Real decoder loading is forbidden in this test'); }
    return { width: 0, height: 0, getContext: () => ({
      drawImage() {}, getImageData() { return { width: 1, height: 1, data: new Uint8ClampedArray(4) }; }
    }) };
  }
};
globalThis.fetch = async () => { fallbackCalls++; throw new Error('Real decoder loading is forbidden in this test'); };
async function run(bytes) {
  bitmapCalls = 0;
  fallbackCalls = 0;
  try {
    await decodeImage('avif', bytes, 32768, 536870912);
    return { accepted: true, bitmapCalls, fallbackCalls };
  } catch (error) { return { accepted: false, bitmapCalls, fallbackCalls, error: error.message }; }
}
const smallResult = await run(small);
const largeResult = await run(large);
console.log(JSON.stringify({ claimedDimensions: [32769, 16], smallBytes: small.length, largeBytes: large.length, smallResult, largeResult }, null, 2));
for (const [name, result] of [['small', smallResult], ['large', largeResult]]) {
  assert.equal(result.bitmapCalls, 0, `${name}: reject before browser decoding`);
  assert.equal(result.fallbackCalls, 0, `${name}: reject before fallback loading`);
  assert.equal(result.accepted, false, `${name}: oversized coded dimensions must be refused`);
  assert.match(result.error, /too large.*32769.*16/i);
}
console.log('PASS AV1 oversized sequence headers reject before decoding at small and 256 MiB offsets');
