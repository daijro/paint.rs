import assert from 'node:assert/strict';
import {requestClipboardImage,writeClipboardImage,writeClipboardText} from '../crates/paintrs/src/platform/web_clipboard.js';
const turns=()=>new Promise(resolve=>setImmediate(resolve));
let writes=[],reads=0,current=[];
globalThis.ClipboardItem=class {constructor(data){this.data=data;}};
Object.defineProperty(globalThis,'navigator',{value:{clipboard:{
 write(items){return new Promise((resolve,reject)=>writes.push({items,resolve:()=>{current=[{types:['image/png'],getType:async()=>items[0].data['image/png']}];resolve();},reject}));},
 writeText(text){return new Promise((resolve,reject)=>writes.push({text,resolve:()=>{current=[{types:['text/plain'],getType:async()=>new Blob([text],{type:'text/plain'})}];resolve();},reject}));},
 read(){reads++;return Promise.resolve(current);}
}},configurable:true});
const a=writeClipboardImage(new Uint8Array([1]));const superseded=writeClipboardImage(new Uint8Array([2]));const latest=writeClipboardImage(new Uint8Array([3]));
await assert.rejects(superseded,/superseded/);assert.equal(writes.length,1,'one active and one latest queued write');
const paste=requestClipboardImage(32768,100);await turns();assert.equal(reads,0,'paste waits for latest queued own write');
writes[0].resolve();await a;await turns();assert.equal(writes.length,2);assert.equal(reads,0);
writes[1].resolve();await latest;assert.deepEqual([...(await paste).bytes],[3]);
const failed=writeClipboardImage(new Uint8Array([4]));const waiting=requestClipboardImage(32768,100);writes[2].reject(Error('denied write'));
await assert.rejects(failed,/denied write/);await assert.rejects(waiting,/denied write/);await turns();
current=[{types:['text/plain'],getType:async()=>new Blob(['external'])}];assert.equal(await requestClipboardImage(32768,100),null,'later external text remains final after failed write');
const text=writeClipboardText('{"polygonList":["0,0,2,0,2,2,0,0"]}');const selection=requestClipboardImage(32768,100,null,true);writes[3].resolve();await text;
assert.match((await selection).text,/polygonList/);assert.equal(await requestClipboardImage(32768,100),null,'BB26 image-only read never returns selection text');
const own=writeClipboardImage(new Uint8Array([5]));const staleEvent=requestClipboardImage(32768,100,new Blob([new Uint8Array([0])],{type:'image/png'}));writes[4].resolve();await own;assert.deepEqual([...(await staleEvent).bytes],[5],'pending copy wins over an older browser paste-event snapshot');
current=[{types:['text/plain','image/png'],getType:async type=>{if(type==='image/png')throw Error('bad image');return new Blob(['{"polygonList":["0,0,2,0,2,2,0,0"]}']);}}];
const mixed=await requestClipboardImage(32768,100,null,true);assert.match(mixed.text,/polygonList/);assert.match(mixed.imageError,/bad image/);
await assert.rejects(requestClipboardImage(32768,100),/bad image/);
console.log('PASS bounded tracked writes, immediate paste ordering, latest queue, failure recovery, selection text, stale event barrier');
