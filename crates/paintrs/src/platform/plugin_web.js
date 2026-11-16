// Installed Paint.NET plug-in .dll files in IndexedDB, so Settings ▸ Plugins ▸ Install… persists
// across reloads and the browser host loads them at start (platform::plugins on the web). One
// object store keyed by file name, holding the raw bytes. Resolve on transaction commit.
const DB = "paintrs-plugins", STORE = "plugins";

function open() {
  return new Promise((resolve, reject) => {
    const r = indexedDB.open(DB, 1);
    r.onupgradeneeded = () => { if (!r.result.objectStoreNames.contains(STORE)) r.result.createObjectStore(STORE, { keyPath: "name" }); };
    r.onblocked = () => reject(new Error("Plug-in storage is blocked by another window"));
    r.onerror = () => reject(r.error || new Error("Could not open plug-in storage"));
    r.onsuccess = () => { const db = r.result; db.onversionchange = () => db.close(); resolve(db); };
  });
}

function read(mapRequest) {
  return async function () {
    const db = await open();
    try {
      return await new Promise((resolve, reject) => {
        const tx = db.transaction(STORE, "readonly"); let result;
        const req = mapRequest(tx.objectStore(STORE));
        req.onsuccess = () => { result = req.result; };
        tx.oncomplete = () => resolve(result);
        tx.onabort = () => reject(tx.error || new Error("Plug-in read aborted"));
        tx.onerror = () => reject(tx.error || new Error("Plug-in read failed"));
      });
    } finally { db.close(); }
  };
}

function write(action) {
  return async function () {
    const db = await open();
    try {
      await new Promise((resolve, reject) => {
        const tx = db.transaction(STORE, "readwrite");
        tx.oncomplete = () => resolve();
        tx.onabort = () => reject(tx.error || new Error("Plug-in write aborted"));
        tx.onerror = () => reject(tx.error || new Error("Plug-in write failed"));
        try { action(tx.objectStore(STORE)); } catch (e) { tx.abort(); reject(e); }
      });
    } finally { db.close(); }
  };
}

export async function pluginList() {
  return (await read((s) => s.getAllKeys())()) || [];
}

export async function pluginAll() {
  const rows = (await read((s) => s.getAll())()) || [];
  // Keep the bytes as typed arrays for the wasm bridge (js_sys::Uint8Array).
  return rows.map((r) => ({ name: r.name, data: r.data instanceof Uint8Array ? r.data : new Uint8Array(r.data) }));
}

// `bytes` is wasm-bindgen's view of Rust memory: copy it before the first await, as memory growth detaches it.
export function pluginPut(name, bytes) {
  const data = bytes.slice();
  return write((s) => s.put({ name, data }))();
}

export async function pluginDelete(name) {
  await write((s) => s.delete(name))();
}
