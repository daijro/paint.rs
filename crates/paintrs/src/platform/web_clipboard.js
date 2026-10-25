// Reads and writes each retain one active job and only the newest queued job.
let active=false,queued=null,writeActive=null,writeQueued=null;
const ownType='web application/x-paintrs-clipboard';
let generation=0,published=null;
function enqueueWrite(value,text=false) {
 let resolve,reject;
 const promise=new Promise((yes,no)=>{resolve=yes;reject=no;});
 // A replacement retires the preceding publication even if the replacement later fails.
 published=null;
 const job={value,text,promise,resolve,reject,generation:++generation,
  token:!text && ClipboardItem.supports?.(ownType) ? crypto.randomUUID() : null};
 if(writeActive) {if(writeQueued)writeQueued.reject(Error('Clipboard write superseded'));writeQueued=job;}
 else startWrite(job);
 return promise;
}
function startWrite(job) {
 writeActive=job;
 const write=token=>{
  const data={'image/png':job.value};
  if(token)data[ownType]=new Blob([token],{type:'application/x-paintrs-clipboard'});
  return navigator.clipboard.write([new ClipboardItem(data)]);
 };
 let result;
 try {result=job.text ? navigator.clipboard.writeText(job.value) : write(job.token);}
 catch(error) {result=Promise.reject(error);}
 // Chrome refuses a custom format (an unsanitized write) without the site's clipboard permission, even in a
 // gesture, and allows the plain PNG: send that once, unowned.
 if(job.token)result=Promise.resolve(result).catch(()=>{job.token=null;return write(null);});
 Promise.resolve(result).then(()=>{
  // The marker and PNG are published atomically; never infer ownership from a later readback.
  if(job.token && job.generation===generation)
   published={token:job.token,blob:job.value,generation:job.generation};
  job.resolve();
 },job.reject).finally(()=>{
  writeActive=null;
  if(writeQueued) {const next=writeQueued;writeQueued=null;startWrite(next);}
 });
}
export function writeClipboardImage(bytes) {
 // Blob snapshots wasm memory now; queued writes never retain a borrowed wasm view.
 return enqueueWrite(new Blob([bytes],{type:'image/png'}));
}
export function writeClipboardText(text) {return enqueueWrite(text,true);}
export function requestClipboardImage(maxSide,maxBytes,blob=null,selectionText=false) {
 const afterWrite=(writeQueued||writeActive)?.promise;
 return new Promise((resolve,reject)=>{
  // A paste event may contain the old clipboard while our Copy is still being published.
  const job={maxSide,maxBytes,blob:afterWrite?null:blob,selectionText,afterWrite,resolve,reject};
  if(active) {if(queued)queued.reject(new Error('Clipboard request superseded'));queued=job;}
  else start(job);
 });
}
async function clipboardPayload(job) {
 const owner=published;
 let items;
 try {items=await navigator.clipboard.read();}
 catch(error) {
  if(published===owner)published=null;
  // An event image is usable without trusting a denied ownership check.
  if(job.blob)return {blob:job.blob,text:null};
  throw error;
 }
 let blob=null,text=null,imageError=null,owned=false;
 for(const item of items) {
  if(!blob && !imageError) {const type=item.types.find(t=>t.startsWith('image/'));if(type)try{
   blob=await item.getType(type);
   if(owner && item.types.includes(ownType))try {
    const marker=await item.getType(ownType);
    owned=marker.size<=64 && await marker.text()===owner.token;
   } catch { /* Missing/unreadable marker leaves the browser image external. */ }
  }catch(error){imageError=String(error);}}
  if(job.selectionText && text===null && item.types.includes('text/plain')) {
   const value=await item.getType('text/plain');
   if(value.size>job.maxBytes)throw Error('Clipboard text too large');
   text=await value.text();
  }
 }
 if(owner && published===owner) {
  if(owned)blob=owner.blob;
  else published=null;
 }
 return {blob,text,imageError};
}
function start(job) {
 active=true;
 // Start access in the caller's gesture when idle. An immediate paste waits for our write.
 const begin=()=>job.blob && !published ? Promise.resolve({blob:job.blob,text:null}) : clipboardPayload(job);
 let read;
 try {read=job.afterWrite ? job.afterWrite.then(begin) : begin();} catch(error) {read=Promise.reject(error);}
 Promise.resolve(read).then(async({blob,text,imageError})=>{
  if(!blob && text===null && !imageError)return null;
  const result={text,imageError};
  if(blob) try {
   if(blob.size>job.maxBytes)throw Error('Clipboard image too large');
   result.format=blob.type.split('/')[1] || 'png';
   result.bytes=new Uint8Array(await blob.arrayBuffer());
  } catch(error) {result.imageError=String(error);}
  if(result.imageError && !job.selectionText)throw Error(result.imageError);
  return result;
 }).then(job.resolve,job.reject).finally(()=>{
  active=false;
  if(queued) {const next=queued;queued=null;start(next);}
 });
}
