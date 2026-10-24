// Task 48w: copy installed npm assets only. No npm invocation or network access.
// Run from any directory: node scripts/copy-web-codecs.mjs <node_modules directory>
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
if (!process.argv[2]) { throw new Error('usage: node scripts/copy-web-codecs.mjs <node_modules directory>'); }
const modules = path.resolve(process.argv[2]);
const packages = [
  { name: '@imagemagick/magick-wasm', version: '0.0.44', target: 'magick', files: {
    'dist/index.js': 'index.js', 'dist/x86/magick.wasm': 'magick.wasm',
    LICENSE: 'LICENSE', NOTICE: 'NOTICE', 'package.json': 'package.json'
  } },
  { name: 'jpegxr', version: '0.3.0', target: 'jpegxr', files: {
    'wasm/jpegxr.js': 'jpegxr.js', 'license.md': 'license.md',
    'license.json': 'license.json', 'package.json': 'package.json'
  } }
];
// Validate the whole input first, so a missing package never leaves a partially copied set.
for (const pkg of packages) {
  const source = path.join(modules, pkg.name);
  const actual = JSON.parse(fs.readFileSync(path.join(source, 'package.json'), 'utf8')).version;
  if (actual !== pkg.version) { throw new Error(`${pkg.name}: expected ${pkg.version}, found ${actual}`); }
  for (const file of Object.keys(pkg.files)) { fs.accessSync(path.join(source, file), fs.constants.R_OK); }
}
for (const pkg of packages) {
  const target = path.join(repo, 'assets/codecs', pkg.target);
  fs.mkdirSync(target, { recursive: true });
  for (const [source, dest] of Object.entries(pkg.files)) {
    fs.copyFileSync(path.join(modules, pkg.name, source), path.join(target, dest));
  }
  console.log(`Copied ${pkg.name} ${pkg.version}, runtime assets and notices`);
}
