// Port of DocumentIO file dialogs/print; R113 distinguishes downloads from confirmed saves.
export const hasActivation = () => globalThis.navigator?.userActivation?.isActive ?? true;
const activationError = () => Object.assign(new Error('ACTIVATION_REQUIRED'), {name:'ActivationRequired'});
function fileInput(multiple, accept, directory=false) {
    return new Promise(resolve => {
        const input=document.createElement('input'); input.type='file'; input.multiple=multiple;
        input.accept=accept; input.webkitdirectory=directory; input.style.display='none';
        const finish=files=>{input.remove();resolve(Array.from(files).map(file=>({name:file.name,mime:file.type,handle:{name:file.name,getFile:async()=>file,downloadOnly:true}})));};
        input.onchange=()=>finish(input.files||[]); input.oncancel=()=>finish([]);
        document.body.append(input); input.click();
    });
}
export function chooseOpen(multiple,accept,allFiles=false) {
    if (!hasActivation()) return Promise.reject(activationError());
    if (typeof window.showOpenFilePicker !== 'function') return fileInput(multiple,accept);
    // R156 (BB27): an explicit ordered filter with All files; older callers keep {multiple}.
    const options=allFiles?{multiple,types:[{description:'Images',accept:{'image/*':accept.split(',')}}],excludeAcceptAllOption:false}:{multiple};
    let promise;
    try {promise=window.showOpenFilePicker(options);} catch(error){promise=Promise.reject(error);}
    return Promise.resolve(promise).then(handles=>handles.map(handle=>({name:handle.name,handle})))
        .catch(error=>{if(error.name==='AbortError')return [];if(['SecurityError','NotAllowedError'].includes(error.name))return fileInput(multiple,accept);throw error;});
}
export function chooseSave(name,description,extensions,types=null) {
    if (!hasActivation()) return Promise.reject(activationError());
    if(typeof window.showSaveFilePicker!=='function')return Promise.resolve({name,handle:null});
    let promise;
    try {promise=window.showSaveFilePicker({suggestedName:name,types:types||[{description,accept:{'application/octet-stream':extensions}}]});}
    catch(error){promise=Promise.reject(error);}
    return Promise.resolve(promise).catch(error=>{if(error instanceof TypeError)return window.showSaveFilePicker({suggestedName:name});throw error;})
        .then(handle=>({name:handle.name,handle}))
        .catch(error=>{if(error.name==='AbortError')return null;if(['SecurityError','NotAllowedError'].includes(error.name))return {name,handle:null};throw error;});
}
// `limit`: the caller's byte bound, never above the 512 MiB decode cap; checked before reading.
export async function readSource(handle,limit=536870912) {
    const max=Math.min(limit,536870912);
    const file=await handle.getFile();
    if(!Number.isFinite(file.size)||file.size<0||file.size>max)throw new Error('File is too large to open.');
    const bytes=new Uint8Array(await file.arrayBuffer());
    if(bytes.byteLength>max)throw new Error('File is too large to open.');
    return bytes;
}
export async function prepareOutput(handle,request=null) {
    if (!handle || typeof handle.createWritable!=='function')return null;
    if (typeof handle.queryPermission==='function' && await handle.queryPermission({mode:'readwrite'})!=='granted') {
        if(!hasActivation())throw activationError();
        if(await handle.requestPermission({mode:'readwrite'})!=='granted')throw new Error('Write permission was not granted.');
    }
    const output=await handle.createWritable();
    if(request){request.output=output;if(request.aborted){await output.abort();throw new Error('Canceled');}}
    return output;
}
export async function writeOutput(output,bytes) {
    try {const owned=bytes.slice();await output.write(owned);await output.close();}
    catch(error){try{await output.abort();}catch{}throw error;}
}
export function abortPreparation(request){request.aborted=true;if(request.output)abortOutput(request.output);}
export function abortOutput(output) {if(output)Promise.resolve(output.abort()).catch(()=>{});}
export function downloadOutput(bytes,name) {
    if(!hasActivation())throw activationError();
    const url=URL.createObjectURL(new Blob([bytes],{type:'application/octet-stream'}));
    const link=document.createElement('a');link.href=url;link.download=name;link.style.display='none';
    document.body.append(link);link.click();link.remove();setTimeout(()=>URL.revokeObjectURL(url),1000);
}
export function printPng(bytes) {
    if(!hasActivation())throw activationError();
    const printWindow=window.open('','_blank');if(!printWindow)throw Error('The print window was blocked by the browser.');
    const url=URL.createObjectURL(new Blob([bytes],{type:'image/png'}));
    printWindow.document.write('<!doctype html><title>Print</title><style>html,body{margin:0;text-align:center}img{max-width:100%;height:auto}</style><img id="printImage" alt="">');printWindow.document.close();
    const image=printWindow.document.getElementById('printImage');
    image.onload=()=>{printWindow.focus();printWindow.print();URL.revokeObjectURL(url);};image.onerror=()=>URL.revokeObjectURL(url);image.src=url;
}
export function paletteFiles(){
    if(!hasActivation())return Promise.reject(activationError());
    if(typeof window.showDirectoryPicker!=='function')return fileInput(true,'.txt,text/plain',true);
    return window.showDirectoryPicker({mode:'read'}).then(async directory=>{
        const files=[];for await(const handle of directory.values())if(handle.kind==='file'&&handle.name.toLowerCase().endsWith('.txt'))files.push({name:handle.name,handle});return files;
    }).catch(error=>{if(error.name==='AbortError')return [];throw error;});
}
