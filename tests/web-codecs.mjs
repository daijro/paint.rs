// Task 48w: real bundled decoders in Chromium, before Task 50 supplies app file-open routing.
// Run: node tests/web-codecs.mjs [Trunk dist directory]
// Task 57 carry: open a.heic in the actual app after Task 50 and assert a 13 × 9 canvas.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { launch } from './browser.mjs';

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const root = process.argv[2] ? path.resolve(process.argv[2]) : repo;
const shim = process.argv[2]
  ? fs.readdirSync(root, { recursive: true }).find(f => f.endsWith('/web_shim.js'))
  : 'crates/paintrs/src/platform/web_shim.js';
assert.ok(shim, 'Trunk must package the wasm-bindgen decoder module');
const fixtures = name => Array.from(fs.readFileSync(path.join(repo, 'tests/fixtures', name)));
const requests = [];
let block = '';
const server = http.createServer((req, res) => {
  const url = new URL(req.url, 'http://localhost');
  requests.push(url.pathname);
  if (!url.pathname.startsWith('/paintrs/')) { res.writeHead(404).end(); return; }
  const rel = decodeURIComponent(url.pathname.slice('/paintrs/'.length));
  if (!rel) { res.setHeader('Content-Type', 'text/html'); res.end('<!doctype html><title>codec harness</title>'); return; }
  if (block && rel.endsWith(block)) { res.writeHead(503).end('deliberately unavailable codec'); return; }
  const file = path.resolve(root, rel);
  if (!file.startsWith(root + path.sep)) { res.writeHead(403).end(); return; }
  fs.readFile(file, (error, bytes) => {
    if (error) { res.writeHead(404).end(); return; }
    res.setHeader('Content-Type', file.endsWith('.js') ? 'text/javascript' : file.endsWith('.wasm') ? 'application/wasm' : 'application/octet-stream');
    res.end(bytes);
  });
});
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
const origin = `http://127.0.0.1:${server.address().port}`;
const browser = await launch();
const assets = () => requests.filter(r => r.includes('/assets/codecs/'));
async function fresh(forceFallback = false) {
  requests.length = 0;
  const page = await browser.newPage();
  await page.goto(`${origin}/paintrs/`);
  const result = await page.evaluate(async (url, force) => {
    window.bitmapCalls = 0;
    const native = window.createImageBitmap;
    window.createImageBitmap = async (...args) => {
      window.bitmapCalls++;
      if (force) { throw new Error('forced unsupported browser format'); }
      return native(...args);
    };
    try { window.codec = await import(url); return {}; }
    catch (error) { return { error: String(error) }; }
  }, `${origin}/paintrs/${shim}`, forceFallback);
  assert.equal(result.error, undefined, 'decoder shim must load before any codec is requested');
  assert.deepEqual(assets(), [], 'loading the shim must not load fallback assets');
  return page;
}
async function decode(page, format, bytes, budget = 536870912) {
  return page.evaluate(async (f, b, maxBytes) => {
    try {
      const image = await window.codec.decodeImage(f, new Uint8Array(b), 32768, maxBytes);
      return { width: image.width, height: image.height, pixels: Array.from(image.data) };
    } catch (error) { return { error: error.message }; }
  }, format, bytes, budget);
}
function dimensions(result) {
  assert.equal(result.error, undefined);
  assert.deepEqual([result.width, result.height, result.pixels.length], [13, 9, 468]);
}
function gradient(result) {
  dimensions(result);
  assert.ok(result.pixels[24] > 230 && result.pixels[26] < 40, 'red top row');
  assert.ok(result.pixels[442] > 230 && result.pixels[440] < 40, 'blue bottom row');
  assert.ok(result.pixels.filter((_, i) => i % 4 === 3).every(a => a === 255), 'straight opaque alpha');
}
function refused(result, pattern = /too large|limit/i) { assert.match(result.error || '', pattern); }
let passed = 0;
async function test(name, fn) { await fn(); passed++; console.log(`PASS ${name}`); }
try {
  await test('AVIF browser decode leaves fallbacks unloaded', async () => {
    const page = await fresh();
    gradient(await decode(page, 'avif', fixtures('a.avif')));
    assert.equal(await page.evaluate(() => bitmapCalls), 1);
    assert.deepEqual(assets(), []);
    await page.close();
  });
  await test('HEIC fallback decodes real 13 × 9 fixture once under a URL prefix', async () => {
    const page = await fresh(true);
    const results = await Promise.all([decode(page, 'heic', fixtures('a.heic')), decode(page, 'heif', fixtures('a.heic'))]);
    results.forEach(gradient);
    assert.equal(assets().filter(p => p.endsWith('/magick.wasm')).length, 1);
    assert.ok(assets().every(p => p.startsWith('/paintrs/assets/codecs/')));
    assert.equal(assets().filter(p => p.endsWith('/jpegxr.js')).length, 0);
    await page.close();
  });
  await test('AVIF real fallback preserves straight alpha', async () => {
    const page = await fresh(true);
    gradient(await decode(page, 'avif', fixtures('a.avif')));
    const alpha = await decode(page, 'avif', fixtures('avif-lossless-alpha.avif'));
    dimensions(alpha);
    assert.equal(alpha.pixels[3], 77);
    assert.equal(alpha.pixels[51], 255);
    assert.ok(alpha.pixels[27] >= 160 && alpha.pixels[27] <= 175);
    await page.close();
  });
  await test('AVIF item admission handles real grids and display transforms', async () => {
    const page = await fresh(true);
    for (const [name, width, height] of [
      ['avif-grid-2x1.avif', 128, 64], ['avif-clap-irot.avif', 7, 10], ['avif-imir.avif', 13, 9]
    ]) {
      const image = await decode(page, 'avif', fixtures(name));
      assert.equal(image.error, undefined, name);
      assert.deepEqual([image.width, image.height, image.pixels.length], [width, height, width * height * 4], name);
    }
    await page.close();
  });
  await test('JXR real fallback converts BGR and reuses its lazy decoder', async () => {
    const page = await fresh(true);
    for (const format of ['jxr', 'wdp', 'wmp']) {
      const image = await decode(page, format, fixtures('a.jxr'));
      dimensions(image);
      for (let y = 0; y < 9; y++) {
        for (let x = 0; x < 13; x++) {
          assert.deepEqual(image.pixels.slice((y * 13 + x) * 4, (y * 13 + x + 1) * 4), [20 * x, 28 * y, 255 - 20 * x, 255]);
        }
      }
    }
    assert.equal(assets().filter(p => p.endsWith('/jpegxr.js')).length, 1);
    assert.equal(assets().filter(p => p.endsWith('/magick.wasm')).length, 0);
    await page.close();
  });
  await test('input survives detachment of the borrowed wasm memory view during an await', async () => {
    const page = await fresh(true);
    const result = await page.evaluate(async data => {
      const input = new Uint8Array(data);
      const pending = codec.decodeImage('heic', input, 32768, 536870912);
      structuredClone(input, { transfer: [input.buffer] });
      try {
        const image = await pending;
        return { width: image.width, height: image.height, pixels: Array.from(image.data) };
      } catch (error) { return { error: error.message }; }
    }, fixtures('a.heic'));
    gradient(result);
    await page.close();
  });
  await test('unsupported JPEG XR planar alpha rejects instead of silently discarding it', async () => {
    const page = await fresh(true);
    for (const name of ['a-bgra32.jxr', 'a-pbgra32.jxr', 'a-rgba64half.jxr']) {
      refused(await decode(page, 'jxr', fixtures(name)), /unsupported.*planar alpha/i);
    }
    await page.close();
  });
  await test('malformed and unsupported input reject explicitly', async () => {
    const page = await fresh(true);
    for (const format of ['heic', 'avif', 'jxr']) {
      refused(await decode(page, format, [1, 2, 3, 4]), /invalid|corrupt|truncated|unsupported/i);
    }
    refused(await decode(page, 'png', fixtures('a.avif')), /unsupported/i);
    assert.deepEqual(assets(), [], 'invalid headers fail before lazy loads');
    await page.close();
  });
  await test('real decoders reject damaged payloads after valid container preflight', async () => {
    const page = await fresh(true);
    for (const format of ['heic', 'avif', 'jxr']) {
      const bytes = fixtures(`a.${format}`);
      if (format === 'jxr') {
        const at = Buffer.from(bytes).indexOf('WMPHOTO\0');
        bytes[at + 8] = 0; // Invalid codec version, with the size and container intact.
      } else {
        const at = Buffer.from(bytes).indexOf('mdat');
        assert.ok(at > 0);
        if (format === 'heic') { bytes.fill(0, at + 10); }
        else {
          const sequence = at + 6;
          assert.equal(bytes[sequence], 0x0a);
          const frame = sequence + 2 + bytes[sequence + 1];
          assert.equal(bytes[frame] >> 3 & 15, 6);
          bytes.fill(0, frame + 2);
        }
      }
      refused(await decode(page, format, bytes), /invalid|corrupt|decode|error|failed|unsupported/i);
    }
    assert.equal(assets().filter(p => p.endsWith('/magick.wasm')).length, 1);
    assert.equal(assets().filter(p => p.endsWith('/jpegxr.js')).length, 1);
    await page.close();
  });
  await test('encoded bytes, image dimensions, RGBA budget and hidden HEVC dimensions are bounded', async () => {
    const page = await fresh();
    refused(await decode(page, 'heic', fixtures('a.heic'), 32));
    refused(await decode(page, 'heic', fixtures('heic-sps-32768x16384.heic')));
    const huge = fixtures('a.avif');
    const ispe = Buffer.from(huge).indexOf('ispe');
    assert.ok(ispe > 0);
    huge.splice(ispe + 8, 8, 0, 0, 128, 1, 0, 0, 0, 1);
    refused(await decode(page, 'avif', huge));
    const overBudget = fixtures('a.avif');
    overBudget.splice(ispe + 8, 8, 0, 0, 128, 0, 0, 0, 128, 0);
    refused(await decode(page, 'avif', overBudget));
    assert.equal(await page.evaluate(() => bitmapCalls), 0, 'preflight precedes the browser decoder');
    assert.deepEqual(assets(), []);
    await page.close();
  });
  await test('malformed box sizes and item locations fail before any decoder is called', async () => {
    const page = await fresh();
    const base = Buffer.from(fixtures('a.avif'));
    const iloc = base.indexOf('iloc');
    const mutations = [
      bytes => bytes.writeUInt32BE(0xffffffff, 0),
      bytes => { bytes.writeUInt32BE(1, 0); bytes.writeBigUInt64BE(0xffffffffffffffffn, 8); },
      bytes => { bytes[iloc + 8] = 0xf4; },
      bytes => bytes.writeUInt32BE(0xffffffff, iloc + 16),
      bytes => bytes.writeUInt16BE(0xffff, iloc + 20)
    ];
    for (const mutate of mutations) {
      const bytes = Buffer.from(base);
      mutate(bytes);
      refused(await decode(page, 'avif', Array.from(bytes)), /invalid|truncated|unsupported|limit/i);
    }
    assert.equal(await page.evaluate(() => bitmapCalls), 0);
    assert.deepEqual(assets(), []);
    await page.close();
  });
  await test('AV1 coded dimensions cannot hide behind a small ispe', async () => {
    const page = await fresh();
    // Same reduced still-picture reheader as native decode_limits.rs, with literal dimensions.
    function reheader(width, height) {
      const original = Buffer.from(fixtures('a.avif'));
      const mdat = original.indexOf('mdat');
      const start = mdat + 6; // after temporal delimiter 12 00
      assert.equal(original[start], 0x0a);
      const length = original[start + 1];
      const payload = original.subarray(start + 2, start + 2 + length);
      const bits = Array.from(payload).flatMap(v => Array.from({ length: 8 }, (_, i) => v >> (7 - i) & 1));
      assert.equal(bits[4], 1, 'fixture has a reduced still-picture sequence');
      const field = (from, n) => bits.slice(from, from + n).reduce((v, b) => v * 2 + b, 0);
      const oldEnd = 18 + field(10, 4) + 1 + field(14, 4) + 1;
      const replacement = [[15, 4], [15, 4], [width - 1, 16], [height - 1, 16]]
        .flatMap(([v, n]) => Array.from({ length: n }, (_, i) => v >> (n - 1 - i) & 1));
      const repacked = [...bits.slice(0, 10), ...replacement, ...bits.slice(oldEnd)];
      const bytes = Buffer.from(Array.from({ length: repacked.length / 8 }, (_, i) =>
        repacked.slice(i * 8, i * 8 + 8).reduce((v, b) => v * 2 + b, 0)));
      const grow = bytes.length - length;
      const out = Buffer.concat([original.subarray(0, start + 1), Buffer.from([bytes.length]), bytes, original.subarray(start + 2 + length)]);
      out.writeUInt32BE(original.readUInt32BE(mdat - 4) + grow, mdat - 4);
      const extentSize = Buffer.alloc(4);
      extentSize.writeUInt32BE(original.length - mdat - 4);
      const extentAt = out.indexOf(extentSize, out.indexOf('iloc'));
      assert.ok(extentAt > 0 && extentAt < out.indexOf('iinf'));
      out.writeUInt32BE(extentSize.readUInt32BE() + grow, extentAt);
      return Array.from(out);
    }
    // A harmless browser stand-in keeps this regression safe even if admission regresses.
    await page.evaluate(() => { window.createImageBitmap = async () => { window.bitmapCalls++; return { width: 13, height: 9, close() {} }; }; });
    for (const [w, h] of [[32769, 16], [32768, 32768]]) {
      refused(await decode(page, 'avif', reheader(w, h)));
    }
    assert.equal(await page.evaluate(() => bitmapCalls), 0);
    assert.deepEqual(assets(), []);
    await page.close();
  });
  await test('JPEG XR codestream dimensions are checked before output allocation', async () => {
    const page = await fresh(true);
    const huge = fixtures('a.jxr');
    const at = Buffer.from(huge).indexOf('WMPHOTO\0');
    assert.ok(at > 0);
    huge.fill(255, at + 12, at + 16);
    refused(await decode(page, 'jxr', huge));
    await page.close();
  });
  await test('browser bitmaps close on success and limit errors', async () => {
    const page = await fresh();
    const result = await page.evaluate(async bytes => {
      let closed = 0;
      const native = window.createImageBitmap;
      window.createImageBitmap = async (...args) => {
        const bitmap = await native(...args);
        const close = bitmap.close.bind(bitmap);
        bitmap.close = () => { closed++; close(); };
        return bitmap;
      };
      await codec.decodeImage('avif', new Uint8Array(bytes), 32768, 536870912);
      window.createImageBitmap = async () => ({ width: 32769, height: 1, close() { closed++; } });
      try { await codec.decodeImage('avif', new Uint8Array(bytes), 32768, 536870912); }
      catch (error) { return { closed, error: error.message }; }
      return { closed };
    }, fixtures('a.avif'));
    assert.equal(result.closed, 2);
    refused(result);
    assert.deepEqual(assets(), []);
    await page.close();
  });
  for (const broken of ['magick.wasm', 'jpegxr.js']) {
    await test(`failed ${broken} load rejects concurrent and later calls`, async () => {
      const page = await fresh(true);
      block = broken;
      const format = broken === 'magick.wasm' ? 'heic' : 'jxr';
      const bytes = fixtures(format === 'heic' ? 'a.heic' : 'a.jxr');
      const results = await Promise.all([decode(page, format, bytes), decode(page, format, bytes)]);
      results.push(await decode(page, format, bytes));
      for (const result of results) { refused(result, /load|fetch|503/i); }
      assert.equal(assets().filter(p => p.endsWith('/' + broken)).length, 1, 'one shared failed load, no hanging callers');
      block = '';
      await page.close();
    });
  }
  console.log(`${passed} browser codec cases passed; actual-app HEIC open remains Task 57 after Task 50.`);
} finally {
  await browser.close();
  await new Promise(resolve => server.close(resolve));
}
