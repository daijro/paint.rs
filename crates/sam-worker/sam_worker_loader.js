// The MobileSAM worker's start (index.html copies it to the site): wasm-bindgen's JS, then the worker's
// wasm. A wasm that is missing or fails to start would only reject this promise, which the page never
// hears of: say so instead, and the app falls back to GrabCut. A missing JS fires the worker's error event.
importScripts("./sam_worker.js");
wasm_bindgen("./sam_worker_bg.wasm").catch(() => postMessage({ failed: "The MobileSAM worker did not start." }));
