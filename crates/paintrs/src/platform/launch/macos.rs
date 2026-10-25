//! macOS process lifecycle, R130. Keep winit's mandatory delegate intact. Public Apple-event
//! handlers own delivery/quit; sequential run_app_on_demand runs dispose and recreate editors.
//! Sources: desktop/main.js lifecycle; Apple NSAppleEventManager/NSAppleEventDescriptor contracts.
use super::Bridge;
use crate::shell::window_close::Intent;
use objc2::{
    DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, Sel},
    sel,
};
use objc2_app_kit::{NSApplication, NSApplicationWillFinishLaunchingNotification, NSMenuItem};
use objc2_foundation::{
    NSAppleEventDescriptor, NSAppleEventManager, NSAppleEventManagerSuspensionID, NSNotification,
    NSNotificationCenter, NSObject, NSObjectProtocol,
};
use std::{
    cell::{Cell, RefCell},
    ffi::{CStr, OsString},
    io,
    os::unix::ffi::OsStringExt,
    path::PathBuf,
    sync::Arc,
};
use winit::{
    application::ApplicationHandler,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    platform::run_on_demand::EventLoopExtRunOnDemand,
};
// Public Apple Events four-character-code ABI (AERegistry.h/AppleEvents.h), big-endian u32.
const CORE: u32 = u32::from_be_bytes(*b"aevt");
const OPEN: u32 = u32::from_be_bytes(*b"odoc");
const REOPEN: u32 = u32::from_be_bytes(*b"rapp");
const QUIT: u32 = u32::from_be_bytes(*b"quit");
const DIRECT: u32 = u32::from_be_bytes(*b"----");
const LIST: u32 = u32::from_be_bytes(*b"list");
const ERROR: u32 = u32::from_be_bytes(*b"errn");
const CANCELED: i32 = -128; // userCanceledErr, MacErrors.h; cooperative quit refusal.

// Six typed public-selector bridges: cached Foundation gates these on an unavailable optional
// CoreServices binding. Exact signatures follow its generated declarations. No raw AEDesc parsing.
fn register(manager: &NSAppleEventManager, target: &AnyObject, selector: Sel, id: u32) {
    // SAFETY: target is retained by Host for every dispatched event; declared methods take two
    // borrowed descriptor objects and return void. AEEventClass/AEEventID are uint32_t.
    unsafe {
        let _: () = msg_send![manager,setEventHandler:target,andSelector:selector,forEventClass:CORE,andEventID:id];
    }
}
fn remove(manager: &NSAppleEventManager, id: u32) {
    // SAFETY: public ABI takes two uint32_t event codes; called only at final teardown.
    unsafe {
        let _: () = msg_send![manager,removeEventHandlerForEventClass:CORE,andEventID:id];
    }
}
fn parameter(event: &NSAppleEventDescriptor, key: u32) -> Option<Retained<NSAppleEventDescriptor>> {
    // SAFETY: public object result is autoreleased, retained by msg_send's Retained convention.
    unsafe { msg_send![event,paramDescriptorForKeyword:key] }
}
fn descriptor_type(event: &NSAppleEventDescriptor) -> u32 {
    // SAFETY: public DescType return is exactly uint32_t.
    unsafe { msg_send![event, descriptorType] }
}
fn set_parameter(reply: &NSAppleEventDescriptor, value: &NSAppleEventDescriptor, key: u32) {
    // SAFETY: objects remain borrowed for the call; AEKeyword is uint32_t.
    unsafe {
        let _: () = msg_send![reply,setParamDescriptor:value,forKeyword:key];
    }
}
fn suspended_reply(
    manager: &NSAppleEventManager,
    id: NSAppleEventManagerSuspensionID,
) -> *mut NSAppleEventDescriptor {
    // SAFETY: Apple forbids retaining this reply. Borrow only while the token is live and end
    // the borrow before resumeWithSuspensionID invalidates it. No Retained result is constructed.
    unsafe { msg_send![manager,replyAppleEventForSuspensionID:id] }
}
fn error(reply: &NSAppleEventDescriptor) {
    set_parameter(
        reply,
        &NSAppleEventDescriptor::descriptorWithInt32(CANCELED),
        ERROR,
    );
}
struct State {
    bridge: Bridge,
    registered: Cell<bool>,
    token: Cell<Option<NSAppleEventManagerSuspensionID>>,
    item: RefCell<Option<Retained<NSMenuItem>>>,
    failure: RefCell<Option<String>>,
}
define_class!(
    // SAFETY: NSObject has no subclassing requirements; Target has no Drop implementation.
    #[unsafe(super=NSObject)]
    #[thread_kind=MainThreadOnly]
    #[ivars=State]
    struct Target;
    unsafe impl NSObjectProtocol for Target {}
    impl Target {
        #[unsafe(method(willFinish:))]
        fn will_finish(&self,_notification:&NSNotification) {
            self.callback(None,|| {
                let manager=NSAppleEventManager::sharedAppleEventManager();
                register(&manager,self,sel!(open:reply:),OPEN);
                register(&manager,self,sel!(reopen:reply:),REOPEN);
                register(&manager,self,sel!(quit:reply:),QUIT);
                self.ivars().registered.set(true); Ok(())
            });
        }
        #[unsafe(method(open:reply:))]
        fn open(&self,event:&NSAppleEventDescriptor,reply:&NSAppleEventDescriptor) {
            self.callback(Some(reply),|| {
                let list=parameter(event,DIRECT).ok_or_else(||io::Error::other("missing open documents"))?;
                let count=list.numberOfItems();
                if descriptor_type(&list)!=LIST || !(0..=super::wire::MAX_PATHS as isize).contains(&count) { return Err(io::Error::other("invalid open-document list")); }
                let mut paths=Vec::with_capacity(count as usize);
                for index in 1..=count {
                    let url=list.descriptorAtIndex(index).and_then(|d|d.fileURLValue()).ok_or_else(||io::Error::other("file URL coercion failed"))?;
                    if !url.isFileURL() { return Err(io::Error::other("open event contains a non-file URL")); }
                    let pointer=url.fileSystemRepresentation();
                    // SAFETY: NSURL owns this NUL-terminated native path while `url` lives.
                    let bytes=unsafe {CStr::from_ptr(pointer.as_ptr())}.to_bytes();
                    if bytes.len()>super::wire::LIMIT {return Err(io::Error::other("open path exceeds transport bound"));}
                    paths.push(PathBuf::from(OsString::from_vec(bytes.to_vec())));
                }
                self.ivars().bridge.launch(paths)
            });
        }
        #[unsafe(method(reopen:reply:))]
        fn reopen(&self,_event:&NSAppleEventDescriptor,reply:&NSAppleEventDescriptor) {
            self.callback(Some(reply),||self.ivars().bridge.launch(vec![]));
        }
        #[unsafe(method(requestQuit:))]
        fn request_quit(&self,_sender:Option<&AnyObject>) {
            self.callback(None,|| {self.ivars().bridge.request_quit();Ok(())});
        }
        #[unsafe(method(quit:reply:))]
        fn quit(&self,_event:&NSAppleEventDescriptor,reply:&NSAppleEventDescriptor) {
            self.callback(Some(reply),|| {
                if self.ivars().token.get().is_some() { return Err(io::Error::other("quit already suspended")); }
                let manager=NSAppleEventManager::sharedAppleEventManager();
                // Refuse by default if a later suspended-reply lookup unexpectedly fails.
                error(reply);
                let token=manager.suspendCurrentAppleEvent();
                if token.is_null() { return Err(io::Error::other("could not suspend quit")); }
                self.ivars().token.set(Some(token)); self.ivars().bridge.request_quit(); Ok(())
            });
        }
    }
);
impl Target {
    fn new(mtm: MainThreadMarker, bridge: Bridge) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(State {
            bridge,
            registered: Cell::new(false),
            token: Cell::new(None),
            item: RefCell::new(None),
            failure: RefCell::new(None),
        });
        // SAFETY: initialized NSObject subclass, normal init ownership family.
        unsafe { msg_send![super(this), init] }
    }
    fn callback(&self, reply: Option<&NSAppleEventDescriptor>, f: impl FnOnce() -> io::Result<()>) {
        // No Rust panic may unwind through Objective-C event dispatch.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        if !matches!(result, Ok(Ok(()))) {
            if let Some(reply) = reply {
                error(reply);
            } else if let Ok(mut failure) = self.ivars().failure.try_borrow_mut() {
                *failure = Some("native lifecycle callback failed".into());
            }
        }
    }
    fn retarget_quit(&self) -> io::Result<()> {
        let menu = NSApplication::sharedApplication(self.mtm())
            .mainMenu()
            .and_then(|m| m.itemAtIndex(0))
            .and_then(|i| i.submenu())
            .ok_or_else(|| io::Error::other("winit application menu missing"))?;
        let mut found = None;
        for i in 0..menu.numberOfItems() {
            if let Some(item) = menu.itemAtIndex(i)
                && item.action() == Some(sel!(terminate:))
            {
                if found.is_some() {
                    return Err(io::Error::other("ambiguous native Quit menu"));
                }
                found = Some(item);
            }
        }
        let item = found.ok_or_else(|| io::Error::other("winit Quit item missing"))?;
        // SAFETY: explicit weak target is retained by Host; selector exists with AppKit action
        // ABI (optional sender, void). Preserve key equivalent, modifier mask and localized title.
        unsafe {
            item.setTarget(Some(self));
            item.setAction(Some(sel!(requestQuit:)));
        }
        *self.ivars().item.borrow_mut() = Some(item);
        Ok(())
    }
    fn settle(&self, approved: bool) {
        // Take first so completion reentrancy cannot resume the same token twice.
        if let Some(token) = self.ivars().token.take() {
            let manager = NSAppleEventManager::sharedAppleEventManager();
            let pointer = suspended_reply(&manager, token);
            if !pointer.is_null() {
                // SAFETY: token remains live until the immediately following resume.
                set_parameter(
                    unsafe { &*pointer },
                    &NSAppleEventDescriptor::descriptorWithInt32(if approved {
                        0
                    } else {
                        CANCELED
                    }),
                    ERROR,
                );
            }
            // SAFETY: this is the one live suspension token, now removed from its owner.
            unsafe {
                manager.resumeWithSuspensionID(token);
            }
        }
    }
}
thread_local! { static ACTIVE:RefCell<Option<Retained<Target>>>=const {RefCell::new(None)}; }
pub fn settle_canceled_quit() {
    ACTIVE.with(|slot| {
        if let Some(target) = slot.borrow().as_ref()
            && target.ivars().bridge.outcome() == Some(Err(()))
        {
            target.settle(false);
            target.ivars().bridge.clear_outcome();
        }
    });
}
struct Host {
    target: Retained<Target>,
}
impl Drop for Host {
    fn drop(&mut self) {
        self.target.settle(false);
        if let Some(item) = self.target.ivars().item.borrow_mut().take() {
            // SAFETY: final teardown, no editor/event dispatch follows. Never restore terminate:.
            unsafe {
                item.setAction(None);
                item.setTarget(None);
            }
        }
        if self.target.ivars().registered.get() {
            let manager = NSAppleEventManager::sharedAppleEventManager();
            for id in [OPEN, REOPEN, QUIT] {
                remove(&manager, id);
            }
        }
        // SAFETY: remove exactly the retained launch observer before releasing Target.
        unsafe {
            NSNotificationCenter::defaultCenter().removeObserver_name_object(
                &self.target,
                Some(NSApplicationWillFinishLaunchingNotification),
                None,
            );
        }
        ACTIVE.with(|slot| *slot.borrow_mut() = None);
    }
}
struct Idle<'a> {
    target: &'a Target,
    bootstrap: bool,
    error: Option<io::Error>,
}
impl ApplicationHandler<eframe::UserEvent> for Idle<'_> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(ControlFlow::Wait);
        if self.bootstrap {
            self.error = self.target.retarget_quit().err();
            event_loop.exit();
        } else {
            self.check(event_loop);
        }
    }
    fn window_event(
        &mut self,
        _: &ActiveEventLoop,
        _: winit::window::WindowId,
        _: winit::event::WindowEvent,
    ) {
    }
    fn user_event(&mut self, event_loop: &ActiveEventLoop, _: eframe::UserEvent) {
        self.check(event_loop);
    }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.check(event_loop);
    }
}
impl Idle<'_> {
    fn check(&mut self, event_loop: &ActiveEventLoop) {
        let bridge = &self.target.ivars().bridge;
        if bridge.quitting() || bridge.needs_editor() {
            event_loop.exit();
        }
    }
}
fn app_error(e: impl std::error::Error + Send + Sync + 'static) -> eframe::Error {
    eframe::Error::AppCreation(Box::new(e))
}
pub fn run(bridge: Bridge, options: fn() -> eframe::NativeOptions) -> eframe::Result {
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| app_error(io::Error::other("macOS host must run on main thread")))?;
    let mut event_loop = EventLoop::<eframe::UserEvent>::with_user_event()
        .build()
        .map_err(app_error)?;
    let proxy = event_loop.create_proxy();
    bridge.set_idle_wake(Arc::new(move || {
        let _ = proxy.send_event(eframe::UserEvent::RequestRepaint {
            viewport_id: egui::ViewportId::ROOT,
            when: std::time::Instant::now(),
            cumulative_pass_nr: 0,
        });
    }));
    let host = Host {
        target: Target::new(mtm, bridge.clone()),
    };
    // SAFETY: observer is main-thread retained for the entire host lifetime, before first run.
    unsafe {
        NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
            &host.target,
            sel!(willFinish:),
            Some(NSApplicationWillFinishLaunchingNotification),
            None,
        );
    }
    ACTIVE.with(|slot| *slot.borrow_mut() = Some(host.target.clone()));
    let mut bootstrap = Idle {
        target: &host.target,
        bootstrap: true,
        error: None,
    };
    event_loop
        .run_app_on_demand(&mut bootstrap)
        .map_err(app_error)?;
    if let Some(e) = bootstrap.error {
        return Err(app_error(e));
    }
    if !host.target.ivars().registered.get() {
        return Err(app_error(io::Error::other(
            "launch Apple-event handlers were not installed",
        )));
    }
    loop {
        if bridge.try_finish_idle_quit() {
            host.target.settle(true);
            return Ok(());
        }
        if let Some(error) = host.target.ivars().failure.borrow_mut().take() {
            return Err(app_error(io::Error::other(error)));
        }
        bridge.clear_outcome();
        let delivery = bridge.clone();
        let mut editor = eframe::create_native(
            "paint.rs",
            options(),
            Box::new(move |cc| {
                let mut app = crate::PaintApp::new(cc);
                delivery.attach(&cc.egui_ctx);
                app.native_launch = Some(delivery);
                Ok(Box::new(app))
            }),
            &event_loop,
        );
        let result = event_loop.run_app_on_demand(&mut editor);
        drop(editor);
        bridge.detach();
        result.map_err(app_error)?;
        match bridge.outcome() {
            Some(Ok(Intent::ExitProcess)) => {
                host.target.settle(true);
                return Ok(());
            }
            Some(Ok(Intent::CloseRoot)) => {}
            _ => {
                return Err(app_error(io::Error::other(
                    "editor ended without approved close",
                )));
            }
        }
        let mut idle = Idle {
            target: &host.target,
            bootstrap: false,
            error: None,
        };
        event_loop.run_app_on_demand(&mut idle).map_err(app_error)?;
    }
}
