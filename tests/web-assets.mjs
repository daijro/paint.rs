// Task54 asset admission checks the File size before reading its body.
import assert from 'node:assert/strict';
import {assetRead} from '../crates/paintrs/src/platform/asset_web.js';
let reads=0;
await assert.rejects(assetRead({getFile:async()=>({size:26*1024*1024,arrayBuffer:async()=>{reads++;return new ArrayBuffer(0);}})},25*1024*1024),/too large/i);
assert.equal(reads,0);
await assert.rejects(assetRead({size:0,arrayBuffer:async()=>new ArrayBuffer(11)},10),/too large/i);
assert.deepEqual([...await assetRead({size:2,arrayBuffer:async()=>Uint8Array.from([1,2]).buffer},2)],[1,2]);
console.log('asset read admission: passed');
