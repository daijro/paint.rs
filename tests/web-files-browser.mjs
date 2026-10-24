// Real browser gesture + input-file read + async decoder. No test writes to user files.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {launch} from './browser.mjs';
const repo=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const root=process.argv[2]?path.resolve(process.argv[2]):repo;
const find=name=>process.argv[2]?fs.readdirSync(root,{recursive:true}).find(f=>f.endsWith('/'+name)):'crates/paintrs/src/platform/'+name;
const files=find('web_files.js'),codec=find('web_shim.js');assert.ok(files&&codec,'package must include both real modules');
const server=http.createServer((req,res)=>{
 const url=new URL(req.url,'http://localhost');if(!url.pathname.startsWith('/paintrs/')){res.writeHead(404).end();return;}
 const rel=decodeURIComponent(url.pathname.slice(9));if(!rel){res.setHeader('Content-Type','text/html');res.end('<!doctype html><base href="/paintrs/"><button id="go">Continue</button>');return;}
 const file=path.resolve(root,rel);if(!file.startsWith(root+path.sep)){res.writeHead(403).end();return;}
 fs.readFile(file,(e,b)=>{if(e){res.writeHead(404).end();return;}res.setHeader('Content-Type',file.endsWith('.js')?'text/javascript':file.endsWith('.wasm')?'application/wasm':'application/octet-stream');res.end(b);});
});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));const origin=`http://127.0.0.1:${server.address().port}`;
const browser=await launch();
try{
 const page=await browser.newPage();await page.goto(origin+'/paintrs/');
 await page.evaluate(async url=>{window.files=await import(url);},origin+'/paintrs/'+files);
 await page.evaluate(()=>{window.showOpenFilePicker=undefined;document.querySelector('#go').onclick=()=>{window.realGesture=navigator.userActivation.isActive;window.pending=files.chooseOpen(true,'.heic');};});
 const [chooser]=await Promise.all([page.waitForFileChooser(),page.click('#go')]);await chooser.accept([path.join(repo,'tests/fixtures/a.heic')]);
 const decoded=await page.evaluate(async url=>{
  const sources=await pending;const bytes=await files.readSource(sources[0].handle);const codec=await import(url);const result=await codec.decodeImage('heic',bytes,32768,536870912);
  return {gesture:realGesture,name:sources[0].name,width:result.width,height:result.height,length:result.data.length};
 },origin+'/paintrs/'+codec);
 assert.deepEqual(decoded,{gesture:true,name:'a.heic',width:13,height:9,length:468});console.log('PASS real gesture, fallback picker, retained source read and real HEIC decode');
 await page.evaluate(()=>{window.events=[];window.showSaveFilePicker=()=>{events.push(['choose',navigator.userActivation.isActive]);return Promise.resolve({name:'image.png',createWritable:async()=>{events.push('prepare');return {write:async bytes=>events.push(['write',...bytes]),close:async()=>events.push('close'),abort:async()=>events.push('abort')};}});};document.querySelector('#go').onclick=()=>{window.pending=files.chooseSave('image.png','PNG',['.png']).then(async target=>{const output=await files.prepareOutput(target.handle);await files.writeOutput(output,new Uint8Array([1,2,3]));});};});
 await page.click('#go');await page.evaluate(()=>pending);assert.deepEqual(await page.evaluate(()=>events),[['choose',true],'prepare',['write',1,2,3],'close']);console.log('PASS picker starts in trusted gesture and successful prepared stream closes after real bytes');
 const cancel=await page.evaluate(async()=>{window.showSaveFilePicker=()=>Promise.reject(Object.assign(Error('canceled'),{name:'AbortError'}));return files.chooseSave('x.png','PNG',['.png']);});assert.equal(cancel,null);console.log('PASS chooser cancellation');
 const cleanup=await page.evaluate(async()=>{const events=[];try{await files.writeOutput({write:async()=>{throw Error('disk failed')},abort:async()=>events.push('abort')},new Uint8Array([9]));}catch(e){return {error:e.message,events};}});assert.deepEqual(cleanup,{error:'disk failed',events:['abort']});console.log('PASS failed stream aborts and rejects');
 await page.close();
}finally{await browser.close();await new Promise(resolve=>server.close(resolve));}
