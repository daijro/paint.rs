// Browser normalization must not discard our original pixels; external data must never acquire ownership.
import assert from 'node:assert/strict';
import {requestClipboardImage,writeClipboardImage,writeClipboardText} from '../crates/paintrs/src/platform/web_clipboard.js';
const turns=()=>new Promise(resolve=>setImmediate(resolve));
const image=n=>new Blob([Uint8Array.of(n)],{type:'image/png'});
const item=data=>({types:Object.keys(data),getType:async type=>data[type]});
let writes=[],current=[],reads=0,supported=true,denied=false;
globalThis.ClipboardItem=class {static supports(){return supported;}constructor(data){this.data=data;}};
Object.defineProperty(globalThis,'navigator',{value:{clipboard:{
 write(items){return new Promise((resolve,reject)=>writes.push({items,resolve:()=>{current=[item({...items[0].data,'image/png':image(124)})];resolve();},reject}));},
 writeText(text){current=[item({'text/plain':new Blob([text])})];return Promise.resolve();},
 read(){reads++;return denied?Promise.reject(Error('denied')):Promise.resolve(current);}
}},configurable:true});
const bytes=async promise=>[...(await promise).bytes];
const publish=async n=>{const p=writeClipboardImage(Uint8Array.of(n));writes.at(-1).resolve();await p;await turns();};
await publish(123);
assert.deepEqual(await bytes(requestClipboardImage(32768,100)),[123],'successful own publication restores exact original bytes despite browser normalization');
const own=current[0];const count=reads;
assert.deepEqual(await bytes(requestClipboardImage(32768,100,image(124))),[123]);assert.equal(reads,count+1,'own event verifies atomic token once');
current=[item({'image/png':image(99)})];assert.deepEqual(await bytes(requestClipboardImage(32768,100,image(99))),[99],'same-sized external changed pixels remain external');
const externalReads=reads;assert.deepEqual(await bytes(requestClipboardImage(32768,100,image(98))),[98]);assert.equal(reads,externalReads,'ordinary external event has no extra read');
current=[own];assert.deepEqual(await bytes(requestClipboardImage(32768,100)),[124],'observed external replacement retires old token');
await publish(123);current=[];assert.equal(await requestClipboardImage(32768,100),null,'empty is final');
await publish(123);await writeClipboardText('external text');assert.equal(await requestClipboardImage(32768,100),null,'text is final');
await publish(123);const failed=writeClipboardImage(Uint8Array.of(7));writes.at(-1).reject(Error('write denied'));await turns();
assert.deepEqual(Object.keys(writes.at(-1).items[0].data),['image/png'],'a refused marker write retries once with the PNG alone');writes.at(-1).reject(Error('write denied'));await assert.rejects(failed,/denied/);await turns();
assert.deepEqual(await bytes(requestClipboardImage(32768,100)),[124],'failed replacement cannot restore older original');
const a=writeClipboardImage(Uint8Array.of(1)),discarded=writeClipboardImage(Uint8Array.of(2)),b=writeClipboardImage(Uint8Array.of(3));await assert.rejects(discarded,/superseded/);
const paste=requestClipboardImage(32768,100,image(0));writes.at(-1).resolve();await a;await turns();const old=current[0];writes.at(-1).resolve();await b;
assert.deepEqual(await bytes(paste),[3],'immediate paste owns latest successful queued write');
current=[old];assert.deepEqual(await bytes(requestClipboardImage(32768,100)),[124],'superseded successful token cannot claim ownership');
await publish(123);denied=true;assert.deepEqual(await bytes(requestClipboardImage(32768,100,image(124))),[124],'denied event identity read uses provided external image without claiming ownership');denied=false;
assert.deepEqual(await bytes(requestClipboardImage(32768,100)),[124],'denied verification retires identity');
await publish(123);const types=current[0].types;const custom=types.find(t=>t!=='image/png');assert(custom,'browser stand-in preserves atomic custom format');
current=[item({[custom]:await current[0].getType(custom)}),item({'image/png':image(99)})];assert.deepEqual(await bytes(requestClipboardImage(32768,100)),[99],'token must be on the image item itself');
await publish(123);current=[item({'image/png':image(88)})];assert.deepEqual(await bytes(requestClipboardImage(32768,100)),[88],'external replacement before verification is not mistaken for publication readback');
await publish(123);const captured=current[0];let releaseMarker;
current=[{types:captured.types,getType:async type=>type==='image/png'?captured.getType(type):new Promise(resolve=>releaseMarker=()=>captured.getType(type).then(resolve))}];
const late=requestClipboardImage(32768,100);await turns();const replacement=writeClipboardImage(Uint8Array.of(7));writes.at(-1).resolve();await replacement;releaseMarker();
assert.deepEqual(await bytes(late),[124],'retired ownership cannot restore old originals after a newer copy starts');
assert.deepEqual(await bytes(requestClipboardImage(32768,100)),[7],'late read does not retire the newer successful publication');
await publish(123);const fallback=writeClipboardImage(Uint8Array.of(9));writes.at(-1).reject(Object.assign(Error('Write permission denied.'),{name:'NotAllowedError'}));await turns();writes.at(-1).resolve();await fallback;await turns();
const unowned=reads;assert.deepEqual(await bytes(requestClipboardImage(32768,100,image(124))),[124]);assert.equal(reads,unowned,'Chrome refusing the marker without permission: the PNG alone is published, unowned');
supported=false;await publish(123);assert.deepEqual(await bytes(requestClipboardImage(32768,100)),[124],'unsupported custom format publishes PNG but does not claim ownership');
console.log('PASS original-byte ownership, atomic same-item token, external replacement, empty/text, supersession, failure, denial, refused-marker PNG fallback and unsupported formats');
