// Port: AppView.js pointer listeners (paintdotjs b4e6fc5).
// Paint.NET: painting-tools-1 §3.14; browser pressure/coalescing is a platform adapter.
export function installPenInput(canvas) {
    const document = canvas.ownerDocument;
    const window = document.defaultView;
    let owner = null;
    let buttons = 0;
    let compatibility = null;
    let syntheticMotion = false;
    let sequence = 0;
    let pending = 0;
    const sources = new WeakMap();
    const listeners = [];
    const listen = (target, name, callback, capture = true) => {
        target.addEventListener(name, callback, capture);
        listeners.push([target, name, callback, capture]);
    };
    const send = packet => canvas.dispatchEvent(new CustomEvent('paintrs-pen', {detail: JSON.stringify(packet)}));
    const finish = source => {
        if (source && source === pending) {
            canvas.dispatchEvent(new CustomEvent('paintrs-pen', {detail: source}));
            pending = 0;
        }
    };
    const cancel = () => {
        finish(pending);
        if (owner !== null) send({source: ++sequence, kind: 'cancel', buttons, samples: [], ctrl: false, shift: false, alt: false, meta: false});
        owner = null;
        buttons = 0;
        compatibility = null;
    };
    const pointer = event => {
        // A packet may only be accepted during its own source callback.
        finish(pending);
        const sameOwner = owner?.id === event.pointerId && owner?.type === event.pointerType;
        if (owner !== null && !sameOwner && buttons !== 0) {
            if (event.type === 'pointermove') compatibility = [event.clientX, event.clientY, event.timeStamp, false];
            event.stopImmediatePropagation();
            return;
        }
        if (event.pointerType !== 'pen') {
            if (sameOwner || (event.type === 'pointerdown' && canvas.contains(event.target))) {
                buttons = event.buttons & 31;
                owner = buttons ? {id: event.pointerId, type: event.pointerType} : null;
                compatibility = null;
            }
            return;
        }
        if (event.type === 'pointercancel') {
            event.stopImmediatePropagation();
            if (sameOwner) cancel();
            return;
        }
        if (buttons === 0 && !canvas.contains(event.target)) return;
        owner = {id: event.pointerId, type: event.pointerType};
        const rect = canvas.getBoundingClientRect();
        const style = window.getComputedStyle?.(canvas);
        const left = rect.left + (parseFloat(style?.paddingLeft) || 0);
        const top = rect.top + (parseFloat(style?.paddingTop) || 0);
        let samples;
        try { samples = typeof event.getCoalescedEvents === 'function' ? event.getCoalescedEvents() : []; }
        catch { samples = []; }
        if (!samples.length) samples = [event];
        buttons = event.buttons & 31;
        pending = ++sequence;
        sources.set(event, pending);
        const packet = {
            source: pending, kind: event.type, buttons,
            ctrl: !!event.ctrlKey, shift: !!event.shiftKey, alt: !!event.altKey, meta: !!event.metaKey,
            samples: Array.from(samples, p => ({x: p.clientX - left, y: p.clientY - top, pressure: p.pressure, time: p.timeStamp - event.timeStamp}))
        };
        if (event.type === 'pointermove') compatibility = [event.clientX, event.clientY, event.timeStamp];
        // Capture metadata before eframe's synchronous down/up handler. Its ordinary
        // source event retains focus, default-action, modifiers and widget ownership.
        send(packet);
        if (event.type === 'pointermove') {
            // eframe 0.37 listens to mousemove, not pointermove. Feed exactly one through
            // its existing interest/modifier handling, then flush at this source event.
            syntheticMotion = true;
            try {
                const acceptedDefault = document.dispatchEvent(new MouseEvent('mousemove', {
                    bubbles: true, cancelable: true,
                    clientX: event.clientX, clientY: event.clientY,
                    buttons: event.buttons, button: event.button,
                    ctrlKey: event.ctrlKey, shiftKey: event.shiftKey,
                    altKey: event.altKey, metaKey: event.metaKey
                }));
                compatibility.push(!acceptedDefault);
            } finally { syntheticMotion = false; }
            canvas.dispatchEvent(new CustomEvent('paintrs-pen', {detail: ''}));
            finish(sources.get(event));
        }
    };
    for (const name of ['pointerdown', 'pointermove', 'pointerup', 'pointercancel']) listen(document, name, pointer);
    // Installed after eframe on the same targets: stopPropagation still permits these
    // listeners, including rejected buttons whose backend callback never runs logic.
    listen(canvas, 'pointerdown', event => finish(sources.get(event)), false);
    listen(document, 'pointerup', event => finish(sources.get(event)), false);
    listen(document, 'mousemove', event => {
        if (syntheticMotion) return;
        const paired = compatibility;
        compatibility = null;
        if (paired && event.clientX === paired[0] && event.clientY === paired[1] && event.timeStamp === paired[2]) {
            if (paired[3]) event.preventDefault();
            event.stopImmediatePropagation();
        }
    });
    listen(canvas, 'pointerleave', event => {
        if (owner?.type === 'pen' && owner.id === event.pointerId && buttons === 0) cancel();
    });
    // Captured descendant blur is a focus transfer inside the app, not window loss.
    listen(window, 'blur', event => { if (event.target === window) cancel(); });
    return () => {
        pending = 0;
        for (const [target, name, callback, capture] of listeners.splice(0)) target.removeEventListener(name, callback, capture);
        owner = null;
        buttons = 0;
        compatibility = null;
    };
}
