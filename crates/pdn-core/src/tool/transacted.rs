//! Paint.NET's transacted-tool core: the state machine, the changes record, the five history
//! entry kinds, the 500 ms settings window, the toolbar backup and the Enter/Escape rows. The
//! Magic Wand uses it (Task 26h), the move tools (Task 27) and the brush-like tools (Task 31,
//! with the merged [`commit_drawing`]); Paint Bucket and Gradient (Task 32) and Shapes and
//! Line/Curve (Task 33) plug in by implementing [`Transacted`] and forwarding their input `Tool`
//! hooks here; the app itself cancels gestures before history steps and runs the settings-window
//! tick (through [`DynTransacted`]), so no tool has to forward those. Ruling R81: the app ignores
//! Ctrl+Z, Ctrl+Y and Edit ▸ Undo/Redo while a canvas button is held (`AppWorkspace::undo`), so
//! a gesture is cancelled before a step only when no canvas button is held.

use std::any::Any;

use serde_json::Value;

use super::{KeyPress, ToolType};
use crate::app::AppWorkspace;
use crate::history::{HistoryMemento, MementoKind, functions};
use crate::util::color::Color;

/// §7.1 states.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum State {
    #[default]
    Inactive,
    Idle,
    Drawing,
    Dirty,
    Editing,
}

impl State {
    /// A transaction is open: Drawing, Dirty or Editing.
    pub fn is_open(self) -> bool {
        matches!(self, State::Drawing | State::Dirty | State::Editing)
    }

    /// §7.1: the only legal transitions.
    fn can_become(self, to: State) -> bool {
        use State::*;
        matches!(
            (self, to),
            (Inactive, Idle)
                | (Idle, Inactive)
                | (Idle, Drawing)
                | (Idle, Dirty)
                | (Drawing, Idle)
                | (Drawing, Dirty)
                | (Dirty, Idle)
                | (Dirty, Editing)
                | (Editing, Dirty)
        )
    }
}

/// §7.4 step 5: every change moves the deadline to now + 500 ms.
pub const SETTINGS_DEADLINE_MS: f64 = 500.0;
/// §7.4 step 5: the timer that ends a settings edit ticks every 100 ms.
pub const SETTINGS_POLL_MS: f64 = 100.0;

/// A settings edit's window (§7.4 step 5): it ends at the first 100 ms tick at or past the
/// deadline while no mouse button is held, so a slider drag of any length is one edit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SettingsWindow {
    deadline: f64,
    next_tick: f64,
}

impl SettingsWindow {
    /// The first change, at `now`: the timer starts.
    pub fn open(now: f64) -> SettingsWindow {
        SettingsWindow {
            deadline: now + SETTINGS_DEADLINE_MS,
            next_tick: now + SETTINGS_POLL_MS,
        }
    }

    /// A further change.
    pub fn change(&mut self, now: f64) {
        self.deadline = now + SETTINGS_DEADLINE_MS;
    }

    /// Runs the ticks due by `now` (the last one decides): true when the window ends.
    pub fn poll(&mut self, now: f64, button_held: bool) -> bool {
        if now < self.next_tick {
            return false;
        }
        let tick =
            self.next_tick + ((now - self.next_tick) / SETTINGS_POLL_MS).floor() * SETTINGS_POLL_MS;
        self.next_tick = tick + SETTINGS_POLL_MS;
        tick >= self.deadline && !button_held
    }
}

/// The two kinds of Editing (§7.1): a handle drag, or toolbar changes collected over a window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Edit {
    Pointer,
    Settings(SettingsWindow),
}

/// What one tool entry does when stepped (§7.2): each kind becomes the next one on the other
/// stack. Records are the tool's own [`Transacted::Record`].
pub enum Step {
    /// Drawing ended; holds the before-drawing snapshot (the Wand's selection; none otherwise).
    Draw(Option<Box<HistoryMemento>>),
    /// The undone Draw: the snapshot's redo and the record to re-open.
    RedoDraw(Option<Box<HistoryMemento>>, Box<dyn Any>),
    /// An edit: the other record (undo and redo swap it with the current one).
    Edit(Box<dyn Any>),
    /// A commit: the committed record and the inner commit entry (pixels; none for the Wand).
    Finish(Box<dyn Any>, Option<Box<HistoryMemento>>),
    /// The undone Finish: redo commits the current record again.
    RedoFinish,
}

/// A tool history entry: the tool type and the active layer index at its creation; undo and redo
/// switch to that tool, then that layer, then step it.
pub struct ToolEntry {
    pub tool: ToolType,
    pub layer: usize,
    pub step: Step,
}

/// One transaction's state; each transacted tool embeds one.
pub struct Transaction<R> {
    state: State,
    /// The changes record (§7.1) while Drawing, Dirty or Editing.
    record: Option<R>,
    /// Editing: its kind and the record before it.
    edit: Option<(Edit, R)>,
    /// Drawing: the before-drawing snapshot.
    snapshot: Option<HistoryMemento>,
    /// The toolbar's tracked values saved when a record is re-opened from Idle (§7.1).
    backup: Option<SettingsBackup>,
}

struct SettingsBackup {
    values: Vec<(&'static str, Value)>,
    colors: Option<[Color; 2]>,
}

impl<R> Default for Transaction<R> {
    fn default() -> Self {
        Transaction {
            state: State::Inactive,
            record: None,
            edit: None,
            snapshot: None,
            backup: None,
        }
    }
}

impl<R> Transaction<R> {
    pub fn state(&self) -> State {
        self.state
    }

    pub fn record(&self) -> Option<&R> {
        self.record.as_ref()
    }

    /// The record for a tool's own in-place growth while Drawing (a brush stroke's points): no
    /// write-back and no `show`, unlike [`update`].
    pub fn record_mut(&mut self) -> Option<&mut R> {
        self.record.as_mut()
    }

    pub fn edit(&self) -> Option<Edit> {
        self.edit.as_ref().map(|e| e.0)
    }

    /// Drawing, Dirty or Editing.
    pub fn is_open(&self) -> bool {
        self.record.is_some()
    }

    fn set_state(&mut self, to: State) {
        debug_assert!(
            self.state.can_become(to),
            "illegal transacted-tool transition {:?} → {to:?}",
            self.state
        );
        self.state = to;
    }
}

/// What a transacted tool supplies to the core.
pub trait Transacted: 'static {
    /// The changes record: `Clone + PartialEq` over all of its fields (§7.8).
    type Record: Clone + PartialEq + 'static;
    const TOOL: ToolType;
    fn transaction(&mut self) -> &mut Transaction<Self::Record>;
    /// §7.3: the Draw, generic Edit and Finish entry names for `r`.
    fn names(r: &Self::Record) -> [String; 3];
    /// Draw/Edit may use a record-specific shape icon; Finish always keeps the tool icon.
    fn drawing_icon(_r: &Self::Record) -> String {
        Self::TOOL.icon_src()
    }
    /// The §7.3 fold rule's input: the names of the tracked properties that differ.
    fn changed(a: &Self::Record, b: &Self::Record) -> Vec<String>;
    /// The tracked toolbar settings `r` holds, as (key, value): written back to the toolbar
    /// whenever `r` is installed, and the keys of the backup.
    fn settings(r: &Self::Record) -> Vec<(&'static str, Value)>;
    /// Palette tracked by pixel tools, installed and backed up with toolbar values.
    fn colors(_r: &Self::Record) -> Option<[Color; 2]> {
        None
    }
    /// Called on every transition out of Idle, including history reopen.
    fn on_leave_idle(&mut self, _app: &mut AppWorkspace) {}
    /// `r` with the toolbar's current tracked values (a settings edit).
    fn with_toolbar(&mut self, app: &mut AppWorkspace, r: &Self::Record) -> Self::Record;
    /// Shows the transaction's state (its record's selection or overlay, the handles), or
    /// removes it when no record is open.
    fn show(&mut self, app: &mut AppWorkspace);
    /// The inner commit entry (pixel tools: the overlay applied to the layer; §7.2).
    fn commit_inner(&mut self, _app: &mut AppWorkspace) -> Option<HistoryMemento> {
        None
    }
}

/// The object-safe part the app drives (history steps, the set-aside, per-image parking).
pub trait DynTransacted {
    fn state(&mut self) -> State;
    /// A settings window is open: the app needs frames for its 100 ms ticks.
    fn ticking(&mut self) -> bool;
    /// Undo or redo of one of this tool's entries: what it becomes, or the step itself back
    /// (refused, nothing changed) when its §7.2 precondition does not hold.
    fn step_entry(&mut self, app: &mut AppWorkspace, step: Step) -> Result<Step, Step>;
    /// §7.5 case 4 (corrected): cancels a drawing or an edit before a history step, which runs
    /// only while no canvas button is held (R81).
    fn cancel_gesture(&mut self, app: &mut AppWorkspace);
    /// The settings window's 100 ms tick.
    fn poll(&mut self, app: &mut AppWorkspace);
    /// §7.5 case 5: a Dirty transaction is cancelled to Idle and its record handed out.
    fn set_aside(&mut self, app: &mut AppWorkspace) -> Option<Box<dyn Any>>;
    /// …and re-opened afterwards.
    fn restore(&mut self, app: &mut AppWorkspace, record: Box<dyn Any>);
}

impl<T: Transacted> DynTransacted for T {
    fn state(&mut self) -> State {
        self.transaction().state
    }

    fn ticking(&mut self) -> bool {
        matches!(self.transaction().edit(), Some(Edit::Settings(_)))
    }

    fn step_entry(&mut self, app: &mut AppWorkspace, step: Step) -> Result<Step, Step> {
        step_entry(self, app, step)
    }

    fn cancel_gesture(&mut self, app: &mut AppWorkspace) {
        cancel_gesture(self, app);
    }

    fn poll(&mut self, app: &mut AppWorkspace) {
        poll(self, app);
    }

    fn set_aside(&mut self, app: &mut AppWorkspace) -> Option<Box<dyn Any>> {
        (self.transaction().state == State::Dirty)
            .then(|| Box::new(cancel_transaction(self, app)) as Box<dyn Any>)
    }

    fn restore(&mut self, app: &mut AppWorkspace, record: Box<dyn Any>) {
        if let Ok(r) = record.downcast::<T::Record>() {
            reopen(self, app, *r);
        }
    }
}

fn write_settings(app: &mut AppWorkspace, tool: ToolType, values: &[(&'static str, Value)]) {
    for (k, v) in values {
        if app.tool_settings.get(tool, k) != *v {
            app.tool_settings.set(tool, k, v.clone());
        }
    }
}

/// Record write-back without notifications, also used by incremental brush input.
pub fn write_record<T: Transacted>(app: &mut AppWorkspace, r: &T::Record) {
    write_settings(app, T::TOOL, &T::settings(r));
    if let Some([primary, secondary]) = T::colors(r) {
        (app.primary_color, app.secondary_color) = (primary, secondary);
    }
}

fn restore_settings(app: &mut AppWorkspace, tool: ToolType, backup: SettingsBackup) {
    write_settings(app, tool, &backup.values);
    if let Some([primary, secondary]) = backup.colors {
        (app.primary_color, app.secondary_color) = (primary, secondary);
    }
}

fn set_state<T: Transacted>(t: &mut T, app: &mut AppWorkspace, to: State) {
    if t.transaction().state == State::Idle {
        t.on_leave_idle(app);
    }
    t.transaction().set_state(to);
}

/// Installs `r`: its settings go to the toolbar (straight into the settings, so no change handler
/// runs; §7.1 "Write-back"), then the tool shows it.
fn install<T: Transacted>(t: &mut T, app: &mut AppWorkspace, r: T::Record) {
    write_record::<T>(app, &r);
    t.transaction().record = Some(r);
    t.show(app);
}

fn push<T: Transacted>(app: &mut AppWorkspace, name: &str, icon: Option<String>, step: Step) {
    let Some(ws) = app.active_ws_mut() else {
        return;
    };
    let entry = ToolEntry {
        tool: T::TOOL,
        layer: ws.active_layer,
        step,
    };
    let m = HistoryMemento::new(
        name,
        &icon.unwrap_or_else(|| T::TOOL.icon_src()),
        MementoKind::Tool(entry),
    );
    ws.push_memento(m);
}

/// Applies a non-tool memento (a snapshot or an inner entry) to the active document.
fn apply(app: &mut AppWorkspace, m: HistoryMemento) -> HistoryMemento {
    match app.active_ws_mut() {
        Some(ws) => ws.apply_memento(m),
        None => m,
    }
}

/// Inactive → Idle.
pub fn activate<T: Transacted>(t: &mut T) {
    t.transaction().set_state(State::Idle);
}

/// Ends gestures and commits, then Idle → Inactive.
pub fn deactivate<T: Transacted>(t: &mut T, app: &mut AppWorkspace) {
    commit(t, app);
    if t.transaction().state == State::Idle {
        set_state(t, app, State::Inactive);
    }
}

/// Idle → Drawing (the press) with the first record and the before-drawing snapshot.
pub fn begin_drawing<T: Transacted>(
    t: &mut T,
    app: &mut AppWorkspace,
    r: T::Record,
    snapshot: Option<HistoryMemento>,
) {
    set_state(t, app, State::Drawing);
    let tx = t.transaction();
    tx.snapshot = snapshot;
    install(t, app, r);
}

/// A new record while Drawing or in a pointer edit (the pointer moved, or the release).
pub fn update<T: Transacted>(t: &mut T, app: &mut AppWorkspace, r: T::Record) {
    debug_assert!(matches!(
        (t.transaction().state, t.transaction().edit()),
        (State::Drawing, _) | (State::Editing, Some(Edit::Pointer))
    ));
    install(t, app, r);
}

/// Live modifier constraints change a pending record without creating their own edit.
pub fn refresh<T: Transacted>(t: &mut T, app: &mut AppWorkspace, r: T::Record) {
    debug_assert!(t.transaction().state.is_open());
    install(t, app, r);
}

/// Dirty → Editing (a handle press, or the first settings change); the backup is dropped.
pub fn begin_edit<T: Transacted>(t: &mut T, kind: Edit) {
    let tx = t.transaction();
    tx.set_state(State::Editing);
    tx.backup = None;
    tx.edit = tx.record.clone().map(|r| (kind, r));
}

/// "End" (§7.6): Drawing → Dirty with its Draw entry; Editing → Dirty with an Edit entry, named
/// by the fold rule, when the record changed.
pub fn end<T: Transacted>(t: &mut T, app: &mut AppWorkspace) {
    let tx = t.transaction();
    match tx.state {
        State::Drawing => {
            tx.set_state(State::Dirty);
            let snapshot = tx.snapshot.take().map(Box::new);
            let name = tx.record.as_ref().map(T::names).map(|[d, ..]| d);
            let icon = tx.record.as_ref().map(T::drawing_icon);
            t.show(app);
            push::<T>(app, &name.unwrap_or_default(), icon, Step::Draw(snapshot));
        }
        State::Editing => {
            tx.set_state(State::Dirty);
            let (Some((_, before)), Some(r)) = (tx.edit.take(), tx.record.as_ref()) else {
                return;
            };
            if before == *r {
                return;
            }
            let name = match T::changed(&before, r).as_slice() {
                [one] => one.clone(),
                _ => T::names(r)[1].clone(),
            };
            push::<T>(
                app,
                &name,
                Some(T::drawing_icon(r)),
                Step::Edit(Box::new(before)),
            );
        }
        _ => {}
    }
}

/// Dirty → Idle through the inner commit entry: the Finish step.
fn finish<T: Transacted>(t: &mut T, app: &mut AppWorkspace) -> Step {
    let inner = t.commit_inner(app).map(Box::new);
    let tx = t.transaction();
    tx.set_state(State::Idle);
    let r = tx.record.take().expect("a Dirty record");
    t.show(app);
    Step::Finish(Box::new(r), inner)
}

/// The commit (§7.2): a gesture ends first, then Dirty → Idle and "Finish …" is pushed, even when
/// nothing changed; the backup is dropped. False when nothing was open.
pub fn commit<T: Transacted>(t: &mut T, app: &mut AppWorkspace) -> bool {
    end(t, app);
    let tx = t.transaction();
    let Some(name) = tx.record.as_ref().map(|r| T::names(r)[2].clone()) else {
        return false;
    };
    tx.backup = None;
    let step = finish(t, app);
    push::<T>(app, &name, None, step);
    true
}

/// The brush-like tools' merged commit (painting-tools-1.md §8.1 row "Commit"): Drawing →
/// Dirty → commit → Idle in one call. Only the inner commit entry is
/// pushed (the tool names and icons it), a pixel entry as a `MementoKind::Stroke` with the tool
/// and layer (§8.8: undo makes the layer active, redo switches to the tool and layer), and
/// nothing when it is empty (§8.8 "No change, no entry"). False when nothing was drawing.
pub fn commit_drawing<T: Transacted>(t: &mut T, app: &mut AppWorkspace) -> bool {
    if t.transaction().state != State::Drawing {
        return false;
    }
    t.transaction().set_state(State::Dirty);
    let inner = t.commit_inner(app);
    let tx = t.transaction();
    tx.set_state(State::Idle);
    (tx.record, tx.snapshot, tx.backup) = (None, None, None);
    t.show(app);
    if let (Some(m), Some(ws)) = (inner, app.active_ws_mut()) {
        let kind = match m.kind {
            MementoKind::Bitmap(pixels) => MementoKind::Stroke {
                tool: T::TOOL,
                layer: ws.active_layer,
                pixels,
            },
            kind => kind,
        };
        ws.push_memento(HistoryMemento { kind, ..m });
    }
    true
}

/// Cancels a gesture with no entry (Escape while Drawing; an undo or redo while no canvas button
/// is held, §7.5 case 4 as corrected): Drawing → Idle with the snapshot undone; Editing → Dirty
/// with the pre-edit record re-installed (so the toolbar reverts).
pub fn cancel_gesture<T: Transacted>(t: &mut T, app: &mut AppWorkspace) {
    let tx = t.transaction();
    match tx.state {
        State::Drawing => {
            tx.set_state(State::Idle);
            tx.record = None;
            let snapshot = tx.snapshot.take();
            t.show(app);
            if let Some(s) = snapshot {
                apply(app, s);
            }
        }
        State::Editing => {
            tx.set_state(State::Dirty);
            if let Some((_, before)) = tx.edit.take() {
                install(t, app, before);
            }
        }
        _ => {}
    }
}

/// Dirty → Idle with no entry (undo of Draw, the set-aside): the display goes and a held backup
/// returns to the toolbar.
fn cancel_transaction<T: Transacted>(t: &mut T, app: &mut AppWorkspace) -> T::Record {
    let tx = t.transaction();
    tx.set_state(State::Idle);
    let r = tx.record.take().expect("a Dirty record");
    let backup = tx.backup.take();
    t.show(app);
    if let Some(b) = backup {
        restore_settings(app, T::TOOL, b);
    }
    r
}

/// Idle → Dirty with `r` (undo of Finish, redo of Draw, the set-aside's restore): the toolbar's
/// tracked values are backed up, then `r` is installed.
fn reopen<T: Transacted>(t: &mut T, app: &mut AppWorkspace, r: T::Record) {
    let values = T::settings(&r)
        .into_iter()
        .map(|(k, _)| (k, app.tool_settings.get(T::TOOL, k)))
        .collect();
    let backup = SettingsBackup {
        values,
        colors: T::colors(&r).map(|_| [app.primary_color, app.secondary_color]),
    };
    set_state(t, app, State::Dirty);
    let tx = t.transaction();
    tx.backup = Some(backup);
    install(t, app, r);
}

/// A tracked toolbar setting changed (§7.8 "Settings window"): in Dirty it opens a settings edit
/// and applies at once; inside one it applies and moves the deadline. Idle has no history, and
/// Drawing or a pointer edit ignore it (their next update writes the record back).
pub fn setting_changed<T: Transacted>(t: &mut T, app: &mut AppWorkspace) {
    let now = app.now_ms;
    let tx = t.transaction();
    match tx.state {
        State::Dirty => begin_edit(t, Edit::Settings(SettingsWindow::open(now))),
        State::Editing => match tx.edit.as_mut() {
            Some((Edit::Settings(w), _)) => w.change(now),
            _ => return,
        },
        _ => return,
    }
    let Some(r) = t.transaction().record.clone() else {
        return;
    };
    let r = t.with_toolbar(app, &r);
    install(t, app, r);
}

/// The 100 ms tick (the app runs it every frame): ends a settings edit past its deadline once no mouse
/// button is held.
pub fn poll<T: Transacted>(t: &mut T, app: &mut AppWorkspace) {
    let (now, held) = (app.now_ms, app.mouse_buttons != 0);
    let ended = match t.transaction().edit.as_mut() {
        Some((Edit::Settings(w), _)) => w.poll(now, held),
        _ => false,
    };
    if ended {
        end(t, app);
    }
}

/// The Finish button (§7.6): enabled, and commits, only in Dirty.
pub fn finish_button<T: Transacted>(t: &mut T, app: &mut AppWorkspace) -> bool {
    t.transaction().state == State::Dirty && commit(t, app)
}

/// The Deselect command through the commit-first path (§7.6 note †); the move tools' Enter and
/// Escape run it too (Task 27, ruling R66).
pub(crate) fn deselect(app: &mut AppWorkspace) {
    app.edit_selection(|ws| ws.execute_function(functions::deselect));
}

/// §7.6 Enter and Escape rows. Enter commits in Dirty and
/// otherwise ends, commits and runs Deselect; Escape cancels a drawing, ignores a settings
/// window, commits a handle edit or Dirty, and runs Deselect in Idle. Shift/Ctrl/Alt + Escape is
/// left to the app's generic handler (note §).
pub fn key_press<T: Transacted>(t: &mut T, app: &mut AppWorkspace, key: &KeyPress) -> bool {
    if key.meta {
        return false;
    }
    let (state, edit) = (t.transaction().state, t.transaction().edit());
    match key.key.as_str() {
        "Enter" if !(key.ctrl || key.alt) => {
            commit(t, app);
            if state != State::Dirty {
                deselect(app);
            }
        }
        "Escape" if !(key.shift || key.ctrl || key.alt) => match (state, edit) {
            (State::Drawing, _) => cancel_gesture(t, app),
            (State::Editing, Some(Edit::Settings(_))) => {}
            (State::Editing | State::Dirty, _) => {
                commit(t, app);
            }
            _ => deselect(app),
        },
        _ => return false,
    }
    true
}

/// The §7.2 table for one of this tool's entries: Draw, Edit and Redo-Finish need Dirty,
/// Redo-Draw and Finish need Idle; otherwise the step is refused untouched.
fn step_entry<T: Transacted>(t: &mut T, app: &mut AppWorkspace, step: Step) -> Result<Step, Step> {
    let ready = matches!(
        (&step, t.transaction().state),
        (
            Step::Draw(_) | Step::Edit(_) | Step::RedoFinish,
            State::Dirty
        ) | (Step::RedoDraw(..) | Step::Finish(..), State::Idle)
    );
    if !ready {
        return Err(step);
    }
    let record = |b: Box<dyn Any>| *b.downcast::<T::Record>().expect("this tool's record");
    Ok(match step {
        Step::Draw(snapshot) => {
            let r = cancel_transaction(t, app);
            let redo = snapshot.map(|s| Box::new(apply(app, *s)));
            Step::RedoDraw(redo, Box::new(r))
        }
        Step::RedoDraw(redo, r) => {
            let snapshot = redo.map(|s| Box::new(apply(app, *s)));
            reopen(t, app, record(r));
            Step::Draw(snapshot)
        }
        Step::Edit(other) => {
            let current = t.transaction().record.take().expect("a Dirty record");
            install(t, app, record(other));
            Step::Edit(Box::new(current))
        }
        Step::Finish(r, inner) => {
            if let Some(i) = inner {
                apply(app, *i);
            }
            reopen(t, app, record(r));
            Step::RedoFinish
        }
        Step::RedoFinish => {
            let backup = t.transaction().backup.take();
            let step = finish(t, app);
            if let Some(b) = backup {
                restore_settings(app, T::TOOL, b);
            }
            step
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §7.4 step 5 with a fake clock: ticks every 100 ms from the first change; the window ends at
    /// the first tick ≥ the last change + 500 ms with no button held.
    #[test]
    fn settings_window_timing() {
        let mut w = SettingsWindow::open(0.0);
        assert!(!w.poll(99.0, false));
        assert!(!w.poll(499.0, false));
        assert!(w.poll(500.0, false));
        // A change at 50: deadline 550, so the tick at 500 is too early and 600 ends it.
        let mut w = SettingsWindow::open(0.0);
        w.change(50.0);
        assert!(!w.poll(580.0, false));
        assert!(w.poll(600.0, false));
        // Held: no tick ends it until the button is released; the next tick after does.
        let mut w = SettingsWindow::open(0.0);
        assert!(!w.poll(500.0, true));
        assert!(!w.poll(3000.0, true));
        assert!(!w.poll(3050.0, false));
        assert!(w.poll(3100.0, false));
        // A late frame runs the latest due tick.
        let mut w = SettingsWindow::open(0.0);
        w.change(450.0);
        assert!(w.poll(2000.0, false));
    }

    /// The smallest transacted tool (how Tasks 32 and 33 plug in): a press opens record 1, the
    /// release ends the drawing; `show` logs the state it sees.
    #[derive(Default)]
    struct Probe {
        tx: Transaction<u32>,
        shown: Vec<State>,
    }

    impl Transacted for Probe {
        type Record = u32;
        const TOOL: ToolType = ToolType::PaintBucket;
        fn transaction(&mut self) -> &mut Transaction<u32> {
            &mut self.tx
        }
        fn names(_: &u32) -> [String; 3] {
            ["Draw", "Edit", "Finish"].map(String::from)
        }
        fn changed(_: &u32, _: &u32) -> Vec<String> {
            vec![]
        }
        fn settings(_: &u32) -> Vec<(&'static str, Value)> {
            vec![]
        }
        fn with_toolbar(&mut self, _: &mut AppWorkspace, r: &u32) -> u32 {
            *r
        }
        fn show(&mut self, _: &mut AppWorkspace) {
            self.shown.push(self.tx.state());
        }
    }

    impl crate::tool::Tool for Probe {
        fn tool_type(&self) -> ToolType {
            Self::TOOL
        }
        fn on_activate(&mut self, _: &mut AppWorkspace) {
            activate(self);
        }
        fn on_deactivate(&mut self, app: &mut AppWorkspace) {
            deactivate(self, app);
        }
        fn on_mouse_down(
            &mut self,
            app: &mut AppWorkspace,
            _: f64,
            _: f64,
            _: crate::tool::MouseButton,
            _: &crate::tool::PointerInput,
        ) -> bool {
            begin_drawing(self, app, 1, None);
            true
        }
        fn on_mouse_up(
            &mut self,
            app: &mut AppWorkspace,
            _: f64,
            _: f64,
            _: crate::tool::MouseButton,
            _: &crate::tool::PointerInput,
        ) -> bool {
            end(self, app);
            true
        }
        fn transacted(&mut self) -> Option<&mut dyn DynTransacted> {
            Some(self)
        }
        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }

    /// §7.5 case 4 as corrected (painting-tools-1.md §8.2, R81): the app ignores an undo while the
    /// canvas button is held, so the drawing stays open. §7.5 case 5: undoing a non-tool entry
    /// over a Dirty transaction cancels it to Idle for the step (a pixel tool's overlay leaves the
    /// layer) and re-opens it after; redo leaves it.
    #[test]
    fn non_tool_undo_sets_the_transaction_aside() {
        use crate::testing::{Ev, drive_tool, with_tool};
        let mut app = crate::testing::fresh_doc(4, 4, None);
        app.tool = Some(Box::<Probe>::default());
        app.tool_type = Some(Probe::TOOL);
        app.set_active_document_workspace(Some(0));
        let left = crate::tool::MouseButton::Left;
        // Ignored while the button is held (§7.5 case 4 as corrected): Probe forwards nothing.
        drive_tool(&mut app, Probe::TOOL, &[Ev::Down(1.0, 1.0, left)]);
        assert!(!app.undo());
        let mut state = State::Inactive;
        with_tool::<Probe>(&mut app, |p, _| state = p.tx.state());
        assert_eq!(state, State::Drawing);
        drive_tool(&mut app, Probe::TOOL, &[Ev::Up(1.0, 1.0, left)]);
        let ws = app.active_ws_mut().unwrap();
        let m = HistoryMemento::layer_properties("Layer Properties", "", &ws.document, 0);
        ws.push_memento(m);
        let take = |app: &mut AppWorkspace| {
            let mut log = vec![];
            with_tool::<Probe>(app, |p, _| log = std::mem::take(&mut p.shown));
            log
        };
        take(&mut app);
        assert!(app.undo());
        assert_eq!(take(&mut app), [State::Idle, State::Dirty]);
        assert!(app.redo());
        assert_eq!(take(&mut app), []);
        let names: Vec<_> = app
            .active_ws()
            .unwrap()
            .history
            .undo
            .iter()
            .map(|m| m.name.clone())
            .collect();
        assert_eq!(names[1..], ["Draw", "Layer Properties"]);
    }

    /// A tool entry whose §7.2 precondition fails (Finish while Dirty, Redo-Finish while Idle) is
    /// refused and stays where it was; nothing changes, in any build.
    #[test]
    fn refused_tool_entries_stay_put() {
        use crate::testing::{Ev, drive_tool, with_tool};
        let mut app = crate::testing::fresh_doc(4, 4, None);
        app.tool = Some(Box::<Probe>::default());
        app.tool_type = Some(Probe::TOOL);
        app.set_active_document_workspace(Some(0));
        let left = crate::tool::MouseButton::Left;
        drive_tool(
            &mut app,
            Probe::TOOL,
            &[Ev::Down(1.0, 1.0, left), Ev::Up(1.0, 1.0, left)],
        );
        let entry = |step| {
            let e = ToolEntry {
                tool: Probe::TOOL,
                layer: 0,
                step,
            };
            HistoryMemento::new("Fake", "", MementoKind::Tool(e))
        };
        let ws = app.active_ws_mut().unwrap();
        ws.history
            .push_new_memento(entry(Step::Finish(Box::new(1u32), None)));
        let names = |app: &AppWorkspace| {
            let h = &app.active_ws().unwrap().history;
            let n = |m: &HistoryMemento| m.name.clone();
            (
                h.undo.iter().map(n).collect::<Vec<_>>(),
                h.redo.iter().map(n).collect::<Vec<_>>(),
            )
        };
        let before = names(&app);
        assert!(!app.undo());
        assert_eq!(names(&app), before);
        let mut state = State::Inactive;
        with_tool::<Probe>(&mut app, |p, _| state = p.tx.state());
        assert_eq!(state, State::Dirty);
        // Redo of Redo-Finish needs Dirty: refused while Idle.
        app.active_ws_mut().unwrap().history.undo.pop();
        with_tool::<Probe>(&mut app, |p, app| {
            commit(p, app);
        });
        let ws = app.active_ws_mut().unwrap();
        ws.history.redo.push_front(entry(Step::RedoFinish));
        let before = names(&app);
        assert!(!app.redo());
        assert_eq!(names(&app), before);
        assert_eq!(before.1, ["Fake"]);
    }

    #[test]
    fn legal_transitions() {
        use State::*;
        let all = [Inactive, Idle, Drawing, Dirty, Editing];
        let legal: Vec<_> = all
            .iter()
            .flat_map(|&a| all.iter().map(move |&b| (a, b)))
            .filter(|(a, b)| a.can_become(*b))
            .collect();
        assert_eq!(
            legal,
            [
                (Inactive, Idle),
                (Idle, Inactive),
                (Idle, Drawing),
                (Idle, Dirty),
                (Drawing, Idle),
                (Drawing, Dirty),
                (Dirty, Idle),
                (Dirty, Editing),
                (Editing, Dirty)
            ]
        );
    }
}
