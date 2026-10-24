//! Where the crate decides whether to use more than one thread.
//!
//! Every threading decision is made here, so that the decoding code above it
//! reads the same whether or not the `parallel` feature is on and whether or
//! not the caller asked for a thread count. Two rules hold in both builds:
//!
//! * the output is identical — parallelism only changes *when* a sample is
//!   computed, never what it is, because every unit of work below writes to a
//!   disjoint region;
//! * `threads: Some(1)` runs the serial code itself, not a one-worker pool,
//!   so that "turn it off" and "build without the feature" are the same path.

use alloc::vec::Vec;

use crate::error::Result;

/// How many threads a caller asked for, and what that means here.
///
/// `None` is rayon's global pool, or the pool the call is already inside,
/// which is what almost every caller wants. `Some(1)` is this thread.
/// `Some(n)` is a private pool of `n` threads, built once per decode by
/// [`scope`] and dropped at the end of it.
pub(crate) type Threads = Option<usize>;

/// True when `threads` asks for the serial path. A build without the feature
/// has no other path, so this only exists alongside one.
#[cfg(feature = "parallel")]
#[inline]
const fn is_serial(threads: Threads) -> bool {
    matches!(threads, Some(0 | 1))
}

/// Run a whole decode under the caller's thread choice, and tell it what that
/// choice means from the inside.
///
/// A caller who names a count gets **one** private pool for the entire decode
/// rather than one per stage, and inside it the choice reduces to `None` —
/// "the pool we are already in". A pool this build cannot create is not a
/// reason to refuse an image, so that case falls back to the serial path,
/// which is correct if slower, rather than to a pool the caller did not ask
/// for.
pub(crate) fn scope<R: Send>(threads: Threads, body: impl FnOnce(Threads) -> R + Send) -> R {
    #[cfg(feature = "parallel")]
    match threads {
        Some(n) if n > 1 => {
            return match rayon::ThreadPoolBuilder::new().num_threads(n).build() {
                Ok(pool) => pool.install(|| body(None)),
                Err(_) => body(Some(1)),
            };
        }
        _ => {}
    }
    body(threads)
}

/// Apply `f` to every item, collecting into a `Vec` in the original order.
///
/// Order is preserved whether or not this runs in parallel, because the grid
/// compositor places tiles by their index in this vector.
pub(crate) fn try_map<T, U, F>(items: &[T], threads: Threads, f: F) -> Result<Vec<U>>
where
    T: Sync,
    U: Send,
    F: Fn(&T) -> Result<U> + Send + Sync,
{
    #[cfg(feature = "parallel")]
    if !is_serial(threads) {
        use rayon::prelude::*;
        return items.par_iter().map(&f).collect();
    }
    let _ = threads;
    items.iter().map(f).collect()
}

/// Split `out` into row bands and hand each band to `f` along with the index
/// of its first row.
///
/// `rows_per_band` is chosen by the caller, which knows how much work a row
/// costs; `row_bytes` converts a band back into a row index.
#[cfg_attr(not(feature = "parallel"), allow(unused_variables))]
pub(crate) fn for_each_band<F>(
    out: &mut [u8],
    row_bytes: usize,
    rows_per_band: usize,
    threads: Threads,
    f: F,
) where
    F: Fn(usize, &mut [u8]) + Send + Sync,
{
    #[cfg(feature = "parallel")]
    if !is_serial(threads) && rows_per_band > 0 && row_bytes > 0 {
        use rayon::prelude::*;
        let band = rows_per_band * row_bytes;
        out.par_chunks_mut(band)
            .enumerate()
            .for_each(|(i, dst)| f(i * rows_per_band, dst));
        return;
    }
    f(0, out);
}
