// File service boundary: a picker starts before returning; canceled/failed writes never succeed.
import assert from 'node:assert/strict';
import {chooseSave, prepareOutput, writeOutput, chooseOpen} from '../crates/paintrs/src/platform/web_files.js';
let calls=[];
globalThis.window={showSaveFilePicker(options){calls.push(options.suggestedName);return Promise.resolve({name:'same.png',createWritable:()=>Promise.resolve(stream)});}};
let stream={write:async bytes=>calls.push([...bytes]),close:async()=>calls.push('closed'),abort:async()=>calls.push('aborted')};
const pending=chooseSave('same.png','PNG',['.png']);
assert.deepEqual(calls,['same.png']);
const target=await pending;const output=await prepareOutput(target.handle);assert.equal(calls.length,1);
await writeOutput(output,new Uint8Array([1,2,3]));assert.deepEqual(calls,['same.png',[1,2,3],'closed']);
stream={write:async()=>{throw Error('disk failed')},abort:async()=>calls.push('aborted')};
await assert.rejects(writeOutput(stream,new Uint8Array([2])),/disk failed/);assert.equal(calls.at(-1),'aborted');
window.showSaveFilePicker=()=>Promise.reject(Object.assign(Error('canceled'),{name:'AbortError'}));assert.equal(await chooseSave('x.png','PNG',['.png']),null);
window.showOpenFilePicker=()=>Promise.resolve([{name:'same.png',getFile:async()=>({name:'same.png'})},{name:'same.png',getFile:async()=>({name:'same.png'})}]);
const sources=await chooseOpen(true,'.png');assert.equal(sources.length,2);assert.notEqual(sources[0].handle,sources[1].handle);
const {readSource}=await import('../crates/paintrs/src/platform/web_files.js');
let readCalls=0;
await assert.rejects(readSource({getFile:async()=>({size:536870913,arrayBuffer:async()=>{readCalls++;return new ArrayBuffer(0);}})}),/too large/i);
assert.equal(readCalls,0);
const module=await import('../crates/paintrs/src/platform/web_files.js');
let releaseOutput;let abandoned=0;const request={};
const preparing=prepareOutput({createWritable:()=>new Promise(resolve=>releaseOutput=resolve)},request);
module.abortPreparation(request);releaseOutput({abort:async()=>abandoned++});
await assert.rejects(preparing,/Canceled/);assert.equal(abandoned,1);
// Accepted handles must cross Rust's ImportBatch::begin boundary before any file read.
let delayedReads=0;
const acceptedHandle={name:'accepted.png',getFile:async()=>{delayedReads++;throw Error('late read failure');}};
window.showOpenFilePicker=()=>Promise.resolve([acceptedHandle]);
const accepted=await chooseOpen(true,'.png');
assert.equal(delayedReads,0,'chooser acceptance must not read the file');
assert.equal(accepted[0].name,'accepted.png');assert.equal(accepted[0].handle,acceptedHandle);
await assert.rejects(readSource(accepted[0].handle),/late read failure/);
assert.equal(delayedReads,1,'only the retained-source read attempts loading');
// R156 (BB27): an explicit ordered filter with All files; older callers keep {multiple} only.
let pickerOptions=[];
window.showOpenFilePicker=options=>{pickerOptions.push(options);return Promise.resolve([]);};
assert.deepEqual(await chooseOpen(false,'.png,.jpg,.gif,.bmp',true),[]);
await chooseOpen(true,'.png');
assert.deepEqual(pickerOptions,[
    {multiple:false,types:[{description:'Images',accept:{'image/*':['.png','.jpg','.gif','.bmp']}}],excludeAcceptAllOption:false},
    {multiple:true},
]);
console.log('web file boundary tests passed');
