//! Ported from PhotoCraft crates/algo/src/photo_util.rs @ ff47388 (MIT OR Apache-2.0, used under MIT). Changes: `par_rows` as a generic `rows_mut` with the row length given directly; `chunks_mut`, `chunks2_mut`, `map`, `count`, `each` and `threads` are upstream's par.rs, made `pub` (`photo_util::par_map` is `map(n, true, f)`).
//!
//! rayon is native-only (Global Constraints): every parallel path has a serial wasm twin here.

/// `f(y, row)` for each `stride`-long row of `buf`: rayon on native, serial on wasm. Same result
/// either way.
pub fn rows_mut<T: Send>(buf: &mut [T], stride: usize, f: impl Fn(usize, &mut [T]) + Sync + Send) {
    let stride = stride.max(1);
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        buf.par_chunks_mut(stride)
            .enumerate()
            .for_each(|(y, row)| f(y, row));
    }
    #[cfg(target_arch = "wasm32")]
    buf.chunks_mut(stride)
        .enumerate()
        .for_each(|(y, row)| f(y, row));
}

/// `f(index, chunk)` for each `len`-item chunk of `buf`, in parallel when `par`.
pub fn chunks_mut<T: Send>(
    buf: &mut [T],
    len: usize,
    par: bool,
    f: impl Fn(usize, &mut [T]) + Sync + Send,
) {
    if len == 0 {
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    if par {
        use rayon::prelude::*;
        buf.par_chunks_mut(len)
            .enumerate()
            .for_each(|(i, c)| f(i, c));
        return;
    }
    let _ = par;
    buf.chunks_mut(len).enumerate().for_each(|(i, c)| f(i, c));
}

/// [`chunks_mut`] over two buffers in step: the `i`-th `la`-item chunk of `a` with the `i`-th
/// `lb`-item chunk of `b`.
pub fn chunks2_mut<A: Send, B: Send>(
    a: &mut [A],
    la: usize,
    b: &mut [B],
    lb: usize,
    par: bool,
    f: impl Fn(usize, &mut [A], &mut [B]) + Sync + Send,
) {
    if la == 0 || lb == 0 {
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    if par {
        use rayon::prelude::*;
        a.par_chunks_mut(la)
            .zip(b.par_chunks_mut(lb))
            .enumerate()
            .for_each(|(i, (ca, cb))| f(i, ca, cb));
        return;
    }
    let _ = par;
    a.chunks_mut(la)
        .zip(b.chunks_mut(lb))
        .enumerate()
        .for_each(|(i, (ca, cb))| f(i, ca, cb));
}

/// `[f(0), f(1), …, f(n - 1)]`, in parallel when `par`.
pub fn map<T: Send>(n: usize, par: bool, f: impl Fn(usize) -> T + Sync + Send) -> Vec<T> {
    #[cfg(not(target_arch = "wasm32"))]
    if par {
        use rayon::prelude::*;
        return (0..n).into_par_iter().map(f).collect();
    }
    let _ = par;
    (0..n).map(f).collect()
}

/// How many of `0..n` satisfy `f`.
pub fn count(n: usize, f: impl Fn(usize) -> bool + Sync + Send) -> usize {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        (0..n).into_par_iter().filter(|&i| f(i)).count()
    }
    #[cfg(target_arch = "wasm32")]
    (0..n).filter(|&i| f(i)).count()
}

/// `f(item)` for each of `items`, in parallel when `par`.
pub fn each<T: Send>(items: Vec<T>, par: bool, f: impl Fn(T) + Sync + Send) {
    #[cfg(not(target_arch = "wasm32"))]
    if par {
        use rayon::prelude::*;
        items.into_par_iter().for_each(f);
        return;
    }
    let _ = par;
    items.into_iter().for_each(f);
}

/// Threads the parallel helpers can use (1 on wasm).
pub fn threads() -> usize {
    #[cfg(not(target_arch = "wasm32"))]
    {
        rayon::current_num_threads()
    }
    #[cfg(target_arch = "wasm32")]
    1
}

#[cfg(test)]
mod tests {
    #[test]
    fn map_matches_a_serial_run() {
        let f = |i: usize| (i as f32 * 0.37).sin() + (i % 7) as f32;
        let serial: Vec<f32> = (0..1000).map(f).collect();
        assert_eq!(super::map(1000, true, f), serial);
        assert!(super::map(0, true, f).is_empty());
    }

    #[test]
    fn rows_mut_visits_every_row_once() {
        let mut buf = vec![0.0; 12];
        super::rows_mut(&mut buf, 4, |y, row| {
            row.iter_mut().for_each(|v| *v += y as f32)
        });
        assert_eq!(
            buf,
            [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 2.0]
        );
    }
}
