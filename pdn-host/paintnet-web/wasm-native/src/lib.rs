//! One archive for the browser host. `extern crate` keeps d2d-ffi (d2d1 + dwrite) and wic-ffi
//! (windowscodecs) linked into this staticlib so their C entry points are all present, and this crate
//! provides the COM task-allocator entry points the imaging code imports: on wasm there is no dlopen,
//! so these are load-time symbols the link must satisfy (the desktop satisfies them through the
//! libuser32.so stub instead). The image save paths never call them, so malloc/free and a no-op clear
//! are enough to satisfy the link.
#![allow(clippy::missing_safety_doc)]

extern crate d2dffi;
extern crate wicffi;

use std::os::raw::c_void;

extern "C" {
    fn malloc(n: usize) -> *mut c_void;
    fn free(p: *mut c_void);
}

#[no_mangle]
pub extern "C" fn CoTaskMemAlloc(cb: usize) -> *mut c_void {
    unsafe { malloc(cb) }
}

#[no_mangle]
pub extern "C" fn CoTaskMemFree(p: *mut c_void) {
    unsafe { free(p) }
}

#[no_mangle]
pub extern "C" fn PropVariantClear(_pvar: *mut c_void) -> i32 {
    0
}
