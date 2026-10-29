// AssetPackManager.js / ShapeManager.js IndexedDB wire records, paintdotjs b4e6fc5.
// Resolve mutations on transaction commit, never on the individual request's success.
const names = {packs: "paintdotjs-asset-packs", shapes: "paintdotjs-shapes", plugins: "paintrs-plugins"};
function open(store) {
    return new Promise((resolve,reject) => {
        if (!names[store]) {reject(new Error("Unknown asset store")); return;}
        const request=indexedDB.open(names[store],1);
        let blocked=false;
        request.onupgradeneeded=()=>{if(!request.result.objectStoreNames.contains(store))request.result.createObjectStore(store,{keyPath:"id"});};
        request.onblocked=()=>{blocked=true;reject(new Error("Asset storage is blocked by another window"));};
        request.onerror=()=>reject(request.error||new Error("Could not open asset storage"));
        request.onsuccess=()=>{const db=request.result;db.onversionchange=()=>db.close();if(blocked)db.close();else resolve(db);};
    });
}
function records(db,store) {
    return new Promise((resolve,reject)=>{
        const tx=db.transaction(store,"readonly");let result;
        const request=tx.objectStore(store).getAll();
        request.onsuccess=()=>{result=request.result;};
        request.onerror=()=>reject(request.error||new Error("Could not read asset storage"));
        tx.onabort=()=>reject(tx.error||new Error("Asset read transaction aborted"));
        tx.oncomplete=()=>resolve(result||[]);
    });
}
function wire(record,store) {
    if(store!=="packs")return record;
    let bytes=0;
    if(!Array.isArray(record.files)||record.files.length>1000)throw new Error("Invalid stored pack files");
    const files=record.files.map(file=>{
        let data=file.data;
        if(data instanceof ArrayBuffer)data=new Uint8Array(data);
        else if(ArrayBuffer.isView(data))data=new Uint8Array(data.buffer,data.byteOffset,data.byteLength);
        else if(!Array.isArray(data))throw new Error("Invalid stored image bytes");
        bytes+=data.length;
        if(bytes>60*1024*1024)throw new Error("Stored pack exceeds extracted byte limit");
        return {...file,data:Array.from(data)};
    });
    return {...record,files};
}
export async function assetLoad() {
    const snapshot={packs:[],shapes:[],errors:[]};
    for(const store of ["packs","shapes"]){let db;
        try {db=await open(store);for(const record of await records(db,store)){
            try {snapshot[store].push(wire(record,store));}
            catch(error){snapshot.errors.push(`${store}: ${error.message}`);}
        }} catch(error){snapshot.errors.push(`${store}: ${error.message}`);}
        finally{db?.close();}
    }
    return JSON.stringify(snapshot);
}
export async function assetChange(store,action,id,json) {
    const db=await open(store);
    try {await new Promise((resolve,reject)=>{
        const tx=db.transaction(store,"readwrite");
        tx.oncomplete=()=>resolve();tx.onabort=()=>reject(tx.error||new Error("Asset transaction aborted"));tx.onerror=()=>reject(tx.error||new Error("Asset transaction failed"));
        try {const object=tx.objectStore(store);
            if(action==="put"){const value=JSON.parse(json);if(store==="packs")for(const file of value.files)file.data=new Uint8Array(file.data).buffer;if(store==="plugins")value.data=new Uint8Array(value.data).buffer;object.put(value);}
            else if(action==="delete")object.delete(id);
            else if(action==="clear")object.clear();
            else throw new Error("Unknown asset operation");
        }catch(error){tx.abort();reject(error);}
    });}finally{db.close();}
}

// Installed plug-ins and looks: [{id: file name, data: bytes}] as JSON.
export async function pluginLoad() {
    const db=await open("plugins");
    try {return JSON.stringify((await records(db,"plugins")).map(r=>({id:r.id,data:Array.from(new Uint8Array(r.data))})));}
    finally{db.close();}
}

// Called after the chooser returns retained sources, before allocating their bytes.
export async function assetRead(source,maxBytes) {
    const file=typeof source.getFile==="function" ? await source.getFile() : source;
    if(!Number.isFinite(file.size)||file.size<0||file.size>maxBytes)throw new Error("Asset file is too large");
    const bytes=new Uint8Array(await file.arrayBuffer());
    if(bytes.byteLength>maxBytes)throw new Error("Asset file is too large");
    return bytes;
}
