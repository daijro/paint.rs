//! Shared native close review. Port of desktop/desktop.js close handshake;
//! Paint.NET save-and-close-flows §§2,7. The flow owns documents; this owns disposition.
use egui::{Context, Id, ViewportCommand};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Intent {
    CloseRoot,
    ExitProcess,
}
#[derive(Clone, Debug, Default)]
pub struct State {
    pub requested: Option<Intent>,
    pub reviewing: bool,
    pub outcome: Option<Result<Intent, ()>>,
}
fn key() -> Id {
    Id::unique("native-close-review")
}
pub fn state(ctx: &Context) -> State {
    ctx.data_mut(|d| d.get_temp::<State>(key()).unwrap_or_default())
}
fn update(ctx: &Context, f: impl FnOnce(&mut State)) {
    let mut s = state(ctx);
    f(&mut s);
    ctx.data_mut(|d| d.insert_temp(key(), s));
}
pub fn request(ctx: &Context, intent: Intent) {
    update(ctx, |s| {
        if matches!(s.outcome, Some(Ok(_))) {
            return;
        }
        if s.requested.is_none() || intent == Intent::ExitProcess {
            s.requested = Some(intent);
        }
        s.outcome = None;
    });
    if let Some(requested) = state(ctx).requested
        && let Some(notify) =
            ctx.data_mut(|d| d.get_temp::<RequestNotifier>(Id::unique("native-close-request")))
    {
        (notify.0)(requested);
    }
    ctx.request_repaint();
}
pub fn begin(ctx: &Context) -> bool {
    let mut start = false;
    update(ctx, |s| {
        if s.requested.is_some() && !s.reviewing {
            s.reviewing = true;
            start = true;
        }
    });
    start
}
pub fn finish(ctx: &Context, ok: bool) {
    let intent = state(ctx).requested.unwrap_or(Intent::ExitProcess);
    let outcome = if ok { Ok(intent) } else { Err(()) };
    let accepted = ctx
        .data_mut(|d| d.get_temp::<Notifier>(Id::unique("native-close-notifier")))
        .is_none_or(|notify| (notify.0)(outcome));
    update(ctx, |s| {
        s.outcome = accepted.then_some(outcome);
        s.requested = if accepted { None } else { Some(intent) };
        s.reviewing = false;
    });
    if ok && accepted {
        ctx.send_viewport_cmd(ViewportCommand::Close);
    }
    ctx.request_repaint();
}
#[derive(Clone)]
pub struct RequestNotifier(pub std::sync::Arc<dyn Fn(Intent) + Send + Sync>);
pub fn set_request_notifier(ctx: &Context, notifier: RequestNotifier) {
    ctx.data_mut(|d| d.insert_temp(Id::unique("native-close-request"), notifier));
}
#[derive(Clone)]
pub struct Notifier(pub std::sync::Arc<dyn Fn(Result<Intent, ()>) -> bool + Send + Sync>);
pub fn set_notifier(ctx: &Context, notifier: Notifier) {
    ctx.data_mut(|d| d.insert_temp(Id::unique("native-close-notifier"), notifier));
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quit_upgrades_pending_root_close_and_cancel_clears_it() {
        let ctx = Context::default();
        request(&ctx, Intent::CloseRoot);
        assert!(begin(&ctx));
        request(&ctx, Intent::ExitProcess);
        assert!(!begin(&ctx));
        finish(&ctx, false);
        assert_eq!(state(&ctx).outcome, Some(Err(())));
        request(&ctx, Intent::CloseRoot);
        assert!(begin(&ctx));
        finish(&ctx, true);
        assert_eq!(state(&ctx).outcome, Some(Ok(Intent::CloseRoot)));
    }
}
