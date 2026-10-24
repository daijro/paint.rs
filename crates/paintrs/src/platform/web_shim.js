// Task 48w. Port: DocumentIO.js loadImage and src/portableImageCodec.js decode (paintdotjs
// b4e6fc5). Reference: document-ops-and-formats.md §2.1, §2.13, §5 Task 48; formats-matrix.md §2.
// Only decoding lives here. Rust supplies its existing side and RGBA byte limits.
const codecUrl = file => new URL(`assets/codecs/${file}`, document.baseURI).href;
let magickPromise;
let jpegxrPromise;

function size(width, height, maxSide, maxBytes, bytesPerPixel = 4) {
  if (!(bytesPerPixel > 0)) { throw new Error('Invalid image sample layout'); }
  const length = width * height * bytesPerPixel;
  if (!Number.isSafeInteger(width) || !Number.isSafeInteger(height) || width <= 0 || height <= 0) {
    throw new Error('Invalid image dimensions');
  }
  if (width > maxSide || height > maxSide || !Number.isSafeInteger(length) || length > maxBytes) {
    throw new Error(`Image too large (${width} × ${height})`);
  }
  return length;
}

// H.265 §7.3.2.2 / §7.3.3, the same header guard as io/codecs/native.rs: an ispe can hide a
// much larger coded picture. Read dimensions before either browser or fallback decoding.
function hevcSize(nal) {
  if (nal.length < 2 || (nal[0] >> 1 & 63) !== 33) { return null; }
  if (((nal[0] & 1) << 5 | nal[1] >> 3) !== 0) { throw new Error('Unsupported layered HEVC sequence header'); }
  // Read RBSP bits directly: a hostile in-band SPS must not become a huge JS number array.
  let source = 2;
  let bit = 8;
  let byte = 0;
  let zeros = 0;
  const bits = n => {
    let value = 0;
    for (let i = 0; i < n; i++) {
      if (bit === 8) {
        if (source >= nal.length) { throw new Error('Truncated HEVC sequence header'); }
        byte = nal[source++];
        if (zeros >= 2 && byte === 3) {
          zeros = 0;
          if (source >= nal.length) { throw new Error('Truncated HEVC sequence header'); }
          byte = nal[source++];
        }
        zeros = byte === 0 ? zeros + 1 : 0;
        bit = 0;
      }
      value = value * 2 + (byte >> (7 - bit++) & 1);
    }
    return value;
  };
  const ue = () => {
    let zeros = 0;
    while (bits(1) === 0) { if (++zeros > 31) { throw new Error('Invalid HEVC sequence header'); } }
    return 2 ** zeros - 1 + bits(zeros);
  };
  bits(4);
  const layers = bits(3);
  bits(1); bits(96);
  const present = Array.from({ length: layers }, () => [bits(1), bits(1)]);
  if (layers) { bits(2 * (8 - layers)); }
  for (const [profile, level] of present) { bits((profile ? 88 : 0) + (level ? 8 : 0)); }
  ue();
  if (ue() === 3) { bits(1); }
  return [ue(), ue()];
}

// AV1 §5.3 / §5.5.1: bound every sequence header, including coded sizes hidden by ispe.
// This is the dimension part of io/codecs/native.rs's existing AV1 guard.
function av1Preflight(data, maxSide, maxBytes) {
  let at = 0;
  let sequences = 0;
  while (at < data.length) {
    const header = data[at++];
    if (header & 0x81) { throw new Error('Invalid AV1 OBU header'); }
    if (header & 4) {
      if (at >= data.length) { throw new Error('Truncated AV1 OBU extension'); }
      at++;
    }
    let length = data.length - at;
    if (header & 2) {
      length = 0;
      let shift = 0;
      let byte;
      do {
        if (at >= data.length || shift > 49) { throw new Error('Invalid AV1 OBU length'); }
        byte = data[at++];
        length += (byte & 127) * 2 ** shift;
        shift += 7;
      } while (byte & 128);
    }
    if (!Number.isSafeInteger(length) || length > data.length - at) { throw new Error('Truncated AV1 OBU'); }
    if ((header >> 3 & 15) === 1) {
      const end = (at + length) * 8;
      let bit = at * 8;
      const f = n => {
        if (bit + n > end) { throw new Error('Truncated AV1 sequence header'); }
        let value = 0;
        for (let i = 0; i < n; i++, bit++) { value = value * 2 + (data[Math.floor(bit / 8)] >> (7 - (bit & 7)) & 1); }
        return value;
      };
      f(3); f(1);
      const reduced = f(1);
      if (reduced) { f(5); }
      else {
        let decoderModel = 0;
        let delayBits = 0;
        if (f(1)) {
          f(32); f(32);
          if (f(1)) {
            let zeros = 0;
            while (f(1) === 0) { if (++zeros > 32) { throw new Error('Unsupported AV1 timing interval'); } }
            if (zeros < 32) { f(zeros); }
          }
          decoderModel = f(1);
          if (decoderModel) { delayBits = f(5) + 1; f(32); f(10); }
        }
        const displayDelay = f(1);
        const points = f(5) + 1;
        for (let i = 0; i < points; i++) {
          f(12);
          if (f(5) > 7) { f(1); }
          if (decoderModel && f(1)) { f(delayBits); f(delayBits); f(1); }
          if (displayDelay && f(1)) { f(4); }
        }
      }
      const wb = f(4) + 1;
      const hb = f(4) + 1;
      size(f(wb) + 1, f(hb) + 1, maxSide, maxBytes);
      sequences++;
    }
    at += length;
  }
  if (!sequences) { throw new Error('Invalid AV1 image without a sequence header'); }
}

function heifPreflight(bytes, maxSide, maxBytes, avif) {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const kind = at => String.fromCharCode(...bytes.subarray(at, at + 4));
  let dimensions = 0;
  let boxes = 0;
  const codedItems = new Map();
  const hevcLengths = new Set();
  const locations = new Map();
  let extentsRead = 0;
  let scannedBytes = 0;
  let idat;
  const walk = (start, end, depth) => {
    if (start > end) { throw new Error('Truncated HEIF container'); }
    if (depth > 8) { throw new Error('Invalid HEIF box nesting'); }
    for (let at = start; at < end;) {
      if (end - at < 8 || ++boxes > 65536) { throw new Error('Invalid HEIF boxes'); }
      let length = view.getUint32(at);
      const type = kind(at + 4);
      let header = 8;
      if (length === 1) {
        if (end - at < 16) { throw new Error('Truncated HEIF box'); }
        const large = view.getBigUint64(at + 8);
        if (large > BigInt(end - at)) { throw new Error('Invalid HEIF box length'); }
        length = Number(large); header = 16;
      } else if (length === 0) { length = end - at; }
      if (length < header || length > end - at) { throw new Error('Truncated HEIF box'); }
      const body = at + header;
      const next = at + length;
      if (type === 'meta' || type === 'iprp' || type === 'ipco') {
        walk(body + (type === 'meta' ? 4 : 0), next, depth + 1);
      } else if (type === 'iinf') {
        walk(body + 4 + (bytes[body] === 0 ? 2 : 4), next, depth + 1);
      } else if (type === 'infe') {
        const version = bytes[body];
        if (version === 2 || version === 3) {
          const idBytes = version === 2 ? 2 : 4;
          if (body + 10 + idBytes > next) { throw new Error('Truncated AVIF item info'); }
          const id = idBytes === 2 ? view.getUint16(body + 4) : view.getUint32(body + 4);
          const type = kind(body + 6 + idBytes);
          if (['av01', 'hvc1', 'hev1'].includes(type)) {
            if (codedItems.has(id)) { throw new Error('Invalid duplicate coded item'); }
            codedItems.set(id, type);
          }
        }
      } else if (type === 'idat') {
        if (idat) { throw new Error('Unsupported multiple AVIF item-data boxes'); }
        idat = [body, next];
      } else if (type === 'iloc') {
        let cursor = body;
        const read = n => {
          if (n > 8 || cursor + n > next) { throw new Error('Truncated AVIF item location'); }
          let value = 0;
          for (let i = 0; i < n; i++) { value = value * 256 + bytes[cursor++]; }
          if (!Number.isSafeInteger(value)) { throw new Error('Invalid AVIF item offset'); }
          return value;
        };
        const version = read(1);
        if (version > 2) { throw new Error('Unsupported AVIF item location version'); }
        read(3);
        const sizes = read(1);
        const baseAndIndex = read(1);
        const offsetBytes = sizes >> 4;
        const lengthBytes = sizes & 15;
        const baseBytes = baseAndIndex >> 4;
        const indexBytes = version ? baseAndIndex & 15 : 0;
        const count = read(version === 2 ? 4 : 2);
        if (count > 65536) { throw new Error('AVIF item count exceeds limit'); }
        for (let i = 0; i < count; i++) {
          const id = read(version === 2 ? 4 : 2);
          const method = version ? read(2) & 15 : 0;
          const reference = read(2);
          const base = read(baseBytes);
          const count = read(2);
          const extents = [];
          extentsRead += count;
          if (extentsRead > 65536) { throw new Error('HEIF item extent count exceeds limit'); }
          for (let j = 0; j < count; j++) {
            read(indexBytes);
            extents.push([read(offsetBytes), read(lengthBytes)]);
          }
          if (locations.has(id)) { throw new Error('Invalid duplicate AVIF item location'); }
          if (locations.size >= 65536) { throw new Error('HEIF item count exceeds limit'); }
          locations.set(id, { method, reference, base, extents });
        }
      } else if (type === 'ispe') {
        if (next - body < 12) { throw new Error('Truncated HEIF dimensions'); }
        size(view.getUint32(body + 4), view.getUint32(body + 8), maxSide, maxBytes);
        dimensions++;
      } else if (type === 'hvcC') {
        if (next - body < 23) { throw new Error('Truncated HEVC configuration'); }
        hevcLengths.add((bytes[body + 21] & 3) + 1);
        let sequence = false;
        let cursor = body + 23;
        for (let i = 0; i < bytes[body + 22]; i++) {
          if (cursor + 3 > next) { throw new Error('Truncated HEVC array'); }
          const count = view.getUint16(cursor + 1); cursor += 3;
          for (let n = 0; n < count; n++) {
            if (cursor + 2 > next) { throw new Error('Truncated HEVC NAL'); }
            const length = view.getUint16(cursor); cursor += 2;
            if (cursor + length > next) { throw new Error('Truncated HEVC NAL'); }
            const coded = hevcSize(bytes.subarray(cursor, cursor + length));
            if (coded) { sequence = true; size(...coded, maxSide, maxBytes); }
            cursor += length;
          }
        }
        if (!sequence) { throw new Error('Unsupported HEVC configuration without coded dimensions'); }
      }
      at = next;
    }
  };
  if (bytes.length < 12 || kind(4) !== 'ftyp') { throw new Error('Invalid HEIF/AVIF header'); }
  walk(0, bytes.length, 0);
  if (!dimensions) { throw new Error('Invalid HEIF/AVIF dimensions'); }
  if (!codedItems.size) { throw new Error('Invalid HEIF/AVIF without coded items'); }
  for (const [id, type] of codedItems) {
    if (avif !== (type === 'av01')) { throw new Error('Unsupported mixed HEIF/AVIF codecs'); }
    const item = locations.get(id);
    if (!item || item.reference || item.method > 1 || !item.extents.length || (item.method === 1 && !idat)) {
      throw new Error('Unsupported AVIF item location');
    }
    const [base, end] = item.method === 1 ? idat : [0, bytes.length];
    const parts = [];
    let total = 0;
    for (const [offset, length] of item.extents) {
      const start = base + item.base + offset;
      const count = length || end - start;
      if (!Number.isSafeInteger(start) || start < base || start > end || count > end - start) {
        throw new Error('Truncated AVIF item data');
      }
      total += count;
      scannedBytes += count;
      if (scannedBytes > maxBytes) { throw new Error('HEIF coded items exceed decode byte limit'); }
      if (total > maxBytes) { throw new Error('AVIF item exceeds decode byte limit'); }
      parts.push(bytes.subarray(start, start + count));
    }
    const data = parts.length === 1 ? parts[0] : new Uint8Array(total);
    if (parts.length !== 1) {
      let cursor = 0;
      for (const part of parts) { data.set(part, cursor); cursor += part.length; }
    }
    if (avif) { av1Preflight(data, maxSide, maxBytes); }
    else {
      if (hevcLengths.size !== 1) { throw new Error('Unsupported HEVC NAL length configuration'); }
      const lengthBytes = hevcLengths.values().next().value;
      let cursor = 0;
      while (cursor < data.length) {
        if (cursor + lengthBytes > data.length) { throw new Error('Truncated HEVC image NAL length'); }
        let length = 0;
        for (let i = 0; i < lengthBytes; i++) { length = length * 256 + data[cursor++]; }
        if (length < 2 || length > data.length - cursor) { throw new Error('Truncated HEVC image NAL'); }
        const coded = hevcSize(data.subarray(cursor, cursor + length));
        if (coded) { size(...coded, maxSide, maxBytes); }
        cursor += length;
      }
    }
  }
}

// JPEG XR container (T.832 Annex A) plus the coded dimensions, which can disagree with the IFD.
function jxrPreflight(bytes, maxSide, maxBytes) {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  if (bytes.length < 8 || bytes[0] !== 73 || bytes[1] !== 73 || bytes[2] !== 188) { throw new Error('Invalid JPEG XR header'); }
  const ifd = view.getUint32(4, true);
  if (ifd > bytes.length - 2) { throw new Error('Truncated JPEG XR directory'); }
  const count = view.getUint16(ifd, true);
  if (ifd + 2 + 12 * count > bytes.length) { throw new Error('Truncated JPEG XR directory'); }
  const fields = new Map();
  for (let i = 0; i < count; i++) {
    const at = ifd + 2 + 12 * i;
    const type = view.getUint16(at + 2, true);
    const value = type === 1 ? bytes[at + 8] : type === 3 ? view.getUint16(at + 8, true) : view.getUint32(at + 8, true);
    fields.set(view.getUint16(at, true), value);
  }
  if (fields.has(0xbc80) && fields.has(0xbc81)) { size(fields.get(0xbc80), fields.get(0xbc81), maxSide, maxBytes); }
  for (const [offsetTag, lengthTag] of [[0xbcc0, 0xbcc1], [0xbcc2, 0xbcc3]]) {
    const at = fields.get(offsetTag);
    if (!at) {
      if (offsetTag === 0xbcc0) { throw new Error('Invalid JPEG XR image offset'); }
      continue;
    }
    const length = fields.get(lengthTag) || bytes.length - at;
    if (at + length > bytes.length || length < 16 || String.fromCharCode(...bytes.subarray(at, at + 8)) !== 'WMPHOTO\0') {
      throw new Error('Truncated or invalid JPEG XR codestream');
    }
    const short = bytes[at + 10] >> 7;
    if (!short && length < 20) { throw new Error('Truncated JPEG XR dimensions'); }
    size((short ? view.getUint16(at + 12) : view.getUint32(at + 12)) + 1,
      (short ? view.getUint16(at + 14) : view.getUint32(at + 16)) + 1, maxSide, maxBytes);
  }
  return Boolean(fields.get(0xbcc2));
}

function magick() {
  if (!magickPromise) {
    magickPromise = (async () => {
      const api = await import(codecUrl('magick/index.js'));
      const response = await fetch(codecUrl('magick/magick.wasm'));
      if (!response.ok) { throw new Error(`Could not load bundled image codecs (${response.status})`); }
      await api.initializeImageMagick(new Uint8Array(await response.arrayBuffer()));
      return api;
    })();
  }
  return magickPromise;
}

function jpegxr() {
  if (!jpegxrPromise) {
    jpegxrPromise = new Promise((resolve, reject) => {
      const script = document.createElement('script');
      script.src = codecUrl('jpegxr/jpegxr.js');
      script.onerror = () => { script.remove(); reject(new Error('Could not load bundled JPEG XR codec')); };
      script.onload = async () => {
        try {
          const module = {};
          const codec = await window.jpegxr(module);
          resolve({ module, codec });
        } catch (error) { reject(error); }
        finally { script.remove(); }
      };
      document.head.appendChild(script);
    });
  }
  return jpegxrPromise;
}

async function decodeJxr(bytes, maxSide, maxBytes) {
  const { module, codec } = await jpegxr();
  // jpegxr.decode has no limit parameter. Its supplied Module exposes metadata before copy_all
  // allocates pixels, so bound both the source sample buffer and RGBA before invoking it.
  const input = module._malloc(bytes.length);
  if (!input) { throw new Error('JPEG XR input allocation failed'); }
  let image = 0;
  let info = 0;
  try {
    module.HEAPU8.set(bytes, input);
    image = module._image_decode_with_memory(input, bytes.length);
    if (!image) { throw new Error('Invalid JPEG XR image'); }
    info = module._image_decode_pixel_info(image);
    if (!info) { throw new Error('Invalid JPEG XR pixel format'); }
    const width = module._image_decode_width(image);
    const height = module._image_decode_height(image);
    size(width, height, maxSide, maxBytes);
    size(width, height, maxSide, maxBytes, module._pixel_info_bits_per_pixel(info) / 8);
  } finally {
    if (info) { module._pixel_info_free(info); }
    if (image) { module._image_decode_free(image); }
    module._free(input);
  }
  const decoded = codec.decode(bytes);
  const { width, height, pixelInfo: p } = decoded;
  const length = size(width, height, maxSide, maxBytes);
  if ((p.channels !== 3 && p.channels !== 4) || p.colorFormat !== 'RGB') { throw new Error('Unsupported JPEG XR pixel format'); }
  const sampleBits = { '8': 8, '16': 16, '32Float': 32 }[p.bitDepth];
  if (sampleBits && (p.bitsPerPixel !== p.channels * sampleBits || (p.hasAlpha && p.channels !== 4))) {
    throw new Error('Unsupported JPEG XR sample layout');
  }
  const sourceLength = size(width, height, maxSide, maxBytes, p.bitsPerPixel / 8);
  if (decoded.bytes.length !== sourceLength) { throw new Error('Invalid JPEG XR pixel length'); }
  let samples;
  let sample;
  if (p.bitDepth === '8') { samples = decoded.bytes; sample = i => samples[i] / 255; }
  else if (p.bitDepth === '16') {
    samples = new Uint16Array(decoded.bytes.buffer, decoded.bytes.byteOffset, sourceLength / 2);
    sample = i => samples[i] / 65535;
  } else if (p.bitDepth === '32Float') {
    samples = new Float32Array(decoded.bytes.buffer, decoded.bytes.byteOffset, sourceLength / 4);
    sample = i => Math.max(0, samples[i]) / (1 + Math.max(0, samples[i]));
  } else { throw new Error(`Unsupported JPEG XR bit depth: ${p.bitDepth}`); }
  const rgba = new Uint8ClampedArray(length);
  for (let source = 0, target = 0; target < length; source += p.channels, target += 4) {
    const alpha = p.hasAlpha ? Math.max(0, Math.min(1, sample(source + 3))) : 1;
    const color = channel => {
      const v = sample(source + channel);
      const normalized = Math.max(0, Math.min(1, p.premultipledAlpha && alpha > 0 ? v / alpha : v));
      return Math.round((p.bitDepth === '32Float' ? normalized ** (1 / 2.2) : normalized) * 255);
    };
    rgba[target] = color(p.bgr ? 2 : 0);
    rgba[target + 1] = color(1);
    rgba[target + 2] = color(p.bgr ? 0 : 2);
    rgba[target + 3] = Math.round(alpha * 255);
  }
  return new ImageData(rgba, width, height);
}

export async function decodeImage(format, bytes, maxSide, maxBytes) {
  format = format.toLowerCase();
  if (!['heic', 'heif', 'avif', 'jxr', 'wdp', 'wmp'].includes(format)) { throw new Error(`Unsupported image format: ${format}`); }
  if (!(bytes instanceof Uint8Array) || bytes.length === 0) { throw new Error('Invalid empty image input'); }
  if (bytes.length > maxBytes) { throw new Error('Encoded image exceeds decode byte limit'); }
  if (!Number.isSafeInteger(maxSide) || !Number.isSafeInteger(maxBytes) || maxSide < 1 || maxBytes < 4) {
    throw new Error('Invalid decode limits');
  }
  const jxr = ['jxr', 'wdp', 'wmp'].includes(format);
  let planarAlpha = false;
  if (jxr) { planarAlpha = jxrPreflight(bytes, maxSide, maxBytes); }
  else { heifPreflight(bytes, maxSide, maxBytes, format === 'avif'); }
  // wasm-bindgen lends a view of Rust memory. Keep a snapshot across awaits and memory growth.
  bytes = bytes.slice();
  let bitmap;
  try {
    bitmap = await createImageBitmap(new Blob([bytes], { type: jxr ? 'image/vnd.ms-photo' : `image/${format}` }));
  } catch (_) { /* Browser format unavailable: use only this format's lazy fallback. */ }
  if (bitmap) {
    let canvas;
    try {
      size(bitmap.width, bitmap.height, maxSide, maxBytes);
      canvas = document.createElement('canvas');
      canvas.width = bitmap.width; canvas.height = bitmap.height;
      const ctx = canvas.getContext('2d', { willReadFrequently: true });
      if (!ctx) { throw new Error('Could not create image decode canvas'); }
      ctx.drawImage(bitmap, 0, 0);
      return ctx.getImageData(0, 0, canvas.width, canvas.height);
    } finally {
      bitmap.close();
      if (canvas) { canvas.width = 0; canvas.height = 0; }
    }
  }
  if (jxr) {
    // npm jpegxr never requests the separate alpha plane from jxrlib.
    if (planarAlpha) { throw new Error('Unsupported JPEG XR planar alpha'); }
    return decodeJxr(bytes, maxSide, maxBytes);
  }
  const api = await magick();
  const settings = new api.MagickReadSettings({ format: api.MagickFormat[format === 'avif' ? 'Avif' : format === 'heif' ? 'Heif' : 'Heic'], frameIndex: 0, frameCount: 1 });
  api.ResourceLimits.width = BigInt(maxSide);
  api.ResourceLimits.height = BigInt(maxSide);
  api.ResourceLimits.area = BigInt(Math.floor(maxBytes / 4));
  api.ResourceLimits.memory = BigInt(maxBytes);
  api.ResourceLimits.disk = 0n;
  api.ResourceLimits.maxMemoryRequest = BigInt(maxBytes);
  // ImageMagick also counts temporary conversion images against this limit.
  api.ResourceLimits.listLength = 16n;
  const info = api.MagickImageInfo.create(bytes, settings);
  size(info.width, info.height, maxSide, maxBytes);
  return api.ImageMagick.read(bytes, settings, image => {
    size(image.width, image.height, maxSide, maxBytes);
    image.autoOrient();
    const length = size(image.width, image.height, maxSide, maxBytes);
    image.depth = 8;
    return image.write(api.MagickFormat.Rgba, rgba => {
      if (rgba.length !== length) { throw new Error('Invalid decoded RGBA pixel length'); }
      return new ImageData(new Uint8ClampedArray(rgba), image.width, image.height);
    });
  });
}

// Task52 Diagnostics and existing text-copy clients use the browser's platform clipboard.
export function writeClipboardText(text) {
    if (!navigator.clipboard?.writeText) { throw new Error("Clipboard text writing is unavailable"); }
    return navigator.clipboard.writeText(text);
}
