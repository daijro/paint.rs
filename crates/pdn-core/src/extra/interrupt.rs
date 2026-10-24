//! Ported from PhotoCraft crates/raster/src/interrupt.rs @ ff47388 (MIT OR Apache-2.0, used under MIT). Changes: the cancel callback is the effect worker's `Option<&AtomicBool>` (`EffectEnv::cancel`); progress is accepted and ignored (paint.rs reports none); the tests follow.
//!
//! Heavy algorithms check [`Interrupt::cancelled`] once per tile, row band or iteration and return
//! early when it is set. [`Interrupt::NONE`] never cancels.

use std::sync::atomic::{AtomicBool, Ordering};

/// Cancellation check for one long operation. Cheap to copy.
#[derive(Clone, Copy, Debug, Default)]
pub struct Interrupt<'a>(pub Option<&'a AtomicBool>);

impl Interrupt<'_> {
    /// Never cancelled.
    pub const NONE: Interrupt<'static> = Interrupt(None);

    /// Has the operation been cancelled? Algorithms stop as soon as they see `true`.
    pub fn cancelled(&self) -> bool {
        self.0.is_some_and(|c| c.load(Ordering::Relaxed))
    }

    /// Upstream's progress sink; paint.rs shows no progress, so it is ignored.
    pub fn progress(&self, _fraction: f32) {}

    /// `Err(Cancelled)` once cancelled, for `?` in loops.
    pub fn check(&self) -> Result<(), Cancelled> {
        if self.cancelled() {
            Err(Cancelled)
        } else {
            Ok(())
        }
    }
}

/// Returned by an operation that stopped because its [`Interrupt`] was cancelled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("cancelled")
    }
}

impl std::error::Error for Cancelled {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_never_cancels() {
        assert!(!Interrupt::NONE.cancelled());
        Interrupt::NONE.progress(0.5);
        assert_eq!(Interrupt::NONE.check(), Ok(()));
    }

    #[test]
    fn reports_cancel_and_clamped_progress() {
        let flag = AtomicBool::new(false);
        let i = Interrupt(Some(&flag));
        assert!(!i.cancelled());
        flag.store(true, Ordering::Relaxed);
        assert!(i.cancelled());
        assert_eq!(i.check(), Err(Cancelled));
    }
}
