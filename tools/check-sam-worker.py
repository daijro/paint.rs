#!/usr/bin/env python3
"""Checks the web's MobileSAM worker in headless Chromium, against a site tools/web-dist.sh built:

- insecure: the site on plain HTTP at a host that is not localhost, so no secure context and no Cache
  API: the model loads, uncached (the loading message says so), and a box lookup answers;
- nowasm: the worker's wasm missing: the worker says it did not start, rather than going silent.

Usage: python3 -I tools/check-sam-worker.py [dist] [chromium]. Exits 1 if a check fails.
"""
import json
import os
import subprocess
import sys
import tempfile
import threading
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

DIST = Path(sys.argv[1] if len(sys.argv) > 1 else 'dist').resolve()
CHROMIUM = sys.argv[2] if len(sys.argv) > 2 else 'chromium'
# A page that starts the worker and reports what it says: on `ready`, a 64 × 64 red disc on blue
# goes in and a box around the disc is looked up.
PROBE = """<!doctype html><meta charset="utf-8"><script>
const t0 = performance.now(), seen = [];
const report = r => fetch('report', { method: 'POST', body: JSON.stringify({ secure: isSecureContext, seen, ...r }) });
const late = setTimeout(() => report({ timeout: true }), 150000);
const w = new Worker('sam_worker_loader.js');
w.onerror = e => { clearTimeout(late); report({ error: String(e.message) }); };
w.onmessage = e => {
  const d = e.data;
  if (d.loading === undefined || d.loading === 0) seen.push([Math.round(performance.now() - t0), d]);
  if (d.failed) { clearTimeout(late); report({}); }
  if (d.ready) {
    const px = new Uint8Array(64 * 64 * 4);
    for (let i = 0; i < 64 * 64; i++) {
      const x = i % 64, y = (i / 64) | 0;
      px.set((x - 32) ** 2 + (y - 32) ** 2 <= 144 ? [220, 50, 40, 255] : [60, 110, 180, 255], i * 4);
    }
    w.postMessage({ embed: 1, width: 64, height: 64, rgba: px.buffer }, [px.buffer]);
    w.postMessage({ decode: 1, id: 1, kind: 'rect', x0: 10, y0: 10, x1: 54, y1: 54 });
  }
  if (d.answer !== undefined) {
    clearTimeout(late);
    report({ answer: { bbox: [d.x0, d.y0, d.x1, d.y1], error: d.error } });
  }
};
</script>"""

reports, arrived = {}, {}


class Handler(SimpleHTTPRequestHandler):
    def do_POST(self):
        case = self.path.strip('/').split('/')[0]
        reports[case] = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        self.send_response(204)
        self.end_headers()
        arrived[case].set()

    def log_message(self, *_):
        pass


def site(root, case, wasm):
    """A folder serving the worker's files from dist/ (without its wasm for `nowasm`), and the probe."""
    out = root / case
    out.mkdir()
    names = ['sam_worker_loader.js', 'sam_worker.js', 'models'] + (['sam_worker_bg.wasm'] if wasm else [])
    for name in names:
        os.symlink(DIST / name, out / name)
    (out / 'probe.html').write_text(PROBE)


def run(root, port, case):
    arrived[case] = threading.Event()
    chrome = subprocess.Popen(
        [CHROMIUM, '--headless=new', '--disable-gpu', '--no-first-run', '--no-default-browser-check',
         f'--user-data-dir={root}/profile-{case}', '--host-resolver-rules=MAP sam.test 127.0.0.1',
         f'http://sam.test:{port}/{case}/probe.html'],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        arrived[case].wait(180)
    finally:
        chrome.terminate()
        chrome.wait()
    return reports.get(case, {'timeout': True, 'seen': []})


def main():
    if not (DIST / 'sam_worker_bg.wasm').is_file():
        sys.exit(f'{DIST} has no MobileSAM worker: run tools/web-dist.sh first')
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        site(root, 'insecure', True)
        site(root, 'nowasm', False)
        server = ThreadingHTTPServer(('127.0.0.1', 0), partial(Handler, directory=tmp))
        threading.Thread(target=server.serve_forever, daemon=True).start()
        port = server.server_address[1]
        ok = True

        r = run(root, port, 'insecure')
        loading = [d for _, d in r['seen'] if 'loading' in d]
        answer = r.get('answer') or {}
        passed = (r['secure'] is False if 'secure' in r else False) and loading \
            and loading[0].get('cached') is False and answer.get('error') is None \
            and None not in answer.get('bbox', [None])
        ok &= bool(passed)
        print(f"{'ok  ' if passed else 'FAIL'} insecure: secure context {r.get('secure')}, "
              f"loading {loading[:1]}, answer {answer or r}")

        r = run(root, port, 'nowasm')
        failed = [d['failed'] for _, d in r['seen'] if 'failed' in d]
        passed = bool(failed) and not r.get('timeout')
        ok &= passed
        print(f"{'ok  ' if passed else 'FAIL'} nowasm: {failed or r}")

        server.shutdown()
    sys.exit(0 if ok else 1)


if __name__ == '__main__':
    main()
