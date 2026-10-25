//! Live editing in a Playground session (paint.rs's web build; `paintrs::live`): while a session is
//! connected, Undo and Redo of an image it shares go to the session, which takes back only this
//! person's edits. Nothing here does anything until the session installs its hook.

use crate::app::AppWorkspace;
use std::cell::Cell;

/// Whether the session takes Undo or Redo of the active image: with `Some(redo)` it takes one,
/// with `None` it only says whether it would.
pub type UndoHook = fn(app: &AppWorkspace, take: Option<bool>) -> bool;

thread_local! {
    static UNDO: Cell<Option<UndoHook>> = const { Cell::new(None) };
}

/// Installs the session's Undo hook.
pub fn set_undo_hook(hook: UndoHook) {
    UNDO.set(Some(hook));
}

/// Whether the session takes this Undo (or Redo); with `take` it does.
pub fn takes_undo(app: &AppWorkspace, redo: bool, take: bool) -> bool {
    UNDO.get()
        .is_some_and(|hook| hook(app, take.then_some(redo)))
}
