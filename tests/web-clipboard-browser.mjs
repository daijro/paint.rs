// Real Chromium, fresh profile, no clipboard permission: Copy still reaches the system clipboard. Chrome refuses a
// custom format (an unsanitized write) in a gesture unless the site holds the permission, so the PNG goes alone.
// `node tests/web-clipboard-browser.mjs [dist]`: the built site's snippet, or the source module.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {launch} from './browser.mjs';
const repo=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const root=process.argv[2]?path.resolve(process.argv[2]):repo;
const snippet=process.argv[2]?fs.readdirSync(root,{recursive:true}).find(f=>f.endsWith('/web_clipboard.js')):'crates/paintrs/src/platform/web_clipboard.js';
assert.ok(snippet,'package must include the clipboard module');
const server=http.createServer((req,res)=>{
 const rel=decodeURIComponent(new URL(req.url,'http://localhost').pathname.slice(1));
 if(!rel){res.setHeader('Content-Type','text/html');res.end('<!doctype html><button id="go">Copy</button>');return;}
 const file=path.resolve(root,rel);if(!file.startsWith(root+path.sep)){res.writeHead(403).end();return;}
 fs.readFile(file,(e,b)=>{if(e){res.writeHead(404).end();return;}res.setHeader('Content-Type','text/javascript');res.end(b);});
});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));const origin=`http://127.0.0.1:${server.address().port}`;
const browser=await launch();
try{
 const page=await browser.newPage();await page.goto(origin+'/');
 await page.evaluate(async url=>{
  window.clip=await import(url);
  const canvas=new OffscreenCanvas(3,2);canvas.getContext('2d').fillRect(0,0,1,1);
  // Bytes after IEND, which the browser's re-encode drops: an owned paste is told apart from the clipboard's PNG.
  const encoded=new Uint8Array(await (await canvas.convertToBlob()).arrayBuffer());
  window.png=new Uint8Array([...encoded,1,2,3]);
  // Each Copy runs in the trusted gesture, as the app's does; its outcome is kept for the assertions.
  const copy=()=>{window.copied=clip.writeClipboardImage(png).then(()=>'ok',e=>String(e));};
  document.querySelector('#go').onclick=copy;document.onkeydown=e=>{if(e.ctrlKey&&e.key==='c')copy();};
 },origin+'/'+snippet);
 const state=name=>page.evaluate(async name=>(await navigator.permissions.query({name})).state,name);
 assert.notEqual(await state('clipboard-read'),'granted','a fresh profile holds no clipboard permission');
 const clipboard=()=>page.evaluate(async()=>{
  const items=await navigator.clipboard.read();
  return Promise.all(items.map(async item=>{const blob=await item.getType('image/png'),bitmap=await createImageBitmap(blob);
   return {types:item.types,size:[bitmap.width,bitmap.height],bytes:[...new Uint8Array(await blob.arrayBuffer())]};}));
 });
 const paste=()=>page.evaluate(async()=>[...(await clip.requestClipboardImage(32768,1<<20)).bytes]);
 const png=await page.evaluate(()=>[...png]);
 for(const gesture of ['click','Ctrl+C']){
  if(gesture==='click')await page.click('#go');
  else {await page.keyboard.down('Control');await page.keyboard.press('KeyC');await page.keyboard.up('Control');}
  assert.equal(await page.evaluate(()=>copied),'ok',`${gesture}: Copy resolves, so the app reports no error`);
 }
 // Read only after the copies: the test grants read, as a later Paste's prompt would.
 await browser.defaultBrowserContext().overridePermissions(origin,['clipboard-read']);
 const [plain,...rest]=await clipboard();
 assert.deepEqual([plain.types,plain.size,rest.length],[['image/png'],[3,2],0],'the PNG reached the system clipboard without the marker');
 assert.notDeepEqual(plain.bytes,png,'the browser re-encoded the PNG');
 assert.deepEqual(await paste(),plain.bytes,'without the marker the image pastes as external');
 // With the permission granted the marker goes too, and Paste restores the original bytes.
 await page.click('#go');assert.equal(await page.evaluate(()=>copied),'ok');
 const [owned]=await clipboard();assert.ok(owned.types.includes('web application/x-paintrs-clipboard'),'granted: the marker is written');
 assert.deepEqual(await paste(),png,'granted: the owned copy pastes its original bytes');
 console.log('PASS fresh-profile Copy falls back to the plain PNG (click and Ctrl+C), unowned; granted Copy keeps the marker and ownership');
}finally{await browser.close();await new Promise(resolve=>server.close(resolve));}
