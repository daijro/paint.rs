//! The web's MobileSAM worker. Trunk builds it as a Web Worker (index.html's `data-type="worker"`
//! link, loaded by `sam_worker_loader.js`); paintrs's `platform::sam_worker` starts it the first
//! time Object Selection's toolbar picks MobileSAM. It loads the model from the site and runs
//! the encoder and decoder off the page's thread (`segment::remote::message` is the protocol).
//! Natively there is nothing to run.

#[cfg(target_arch = "wasm32")]
mod worker;

fn main() {
    #[cfg(target_arch = "wasm32")]
    worker::run();
}
