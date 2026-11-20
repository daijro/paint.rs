// The live canvas for a settings window's own custom-drawn control (Liquify's warp brush): paint.rs shows what the
// control draws and sends the user's pointer and key input back to it, through the window the host keeps open while
// paint.rs's dialog is (PluginHost.Canvas*). The control is handled as Windows would handle the window it is part
// of: painting draws it and the controls in it at their places, and input goes to the control under the pointer
// (the one pressed keeps it until released), the wheel to the nearest that takes it, keys to the form first when
// it previews them, then to the canvas.
using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.Drawing.Imaging;
using System.Linq;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Text.Json;
using System.Windows.Forms;

namespace PaintDotNet.Bridge;

internal static class DialogCanvas
{
    private const BindingFlags Instance = BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic;

    /// Runs the work a plug-in handed its own threads, where those do not run by themselves (the browser host has one
    /// thread and queues them), so a replayed event's work is done before the next one or a paint. Set by that host.
    public static Action? RunQueued;

    /// Feeds a code editor's events into it, true when `c` is one (ScintillaBox.Edit, for a Scintilla editor):
    /// {"e": "text", "text", "sel": [start, end]} the whole text after an edit, {"e": "char", "c"} each typed
    /// character, {"e": "sel", "sel"} a moved selection; positions in UTF-16 code units.
    public static Func<Control, JsonElement, bool>? EditControl = ScintillaBox.Edit;

    /// A canvas: a plug-in control that draws (its own OnPaint or OnPaintBackground, or a Paint handler), itself or a
    /// control in it, and takes the pointer (its own mouse handler or a mouse event handler), itself or one in it.
    internal static DialogSettings.Setting? Canvas(Control c, string path, string label)
    {
        if (c.Width <= 0 || c.Height <= 0 || !Tree(c).Any(Draws) || !Tree(c).Any(TakesPointer)) return null;
        return new DialogSettings.Setting
        {
            Key = path + "canvas", Type = "canvas", Label = label, Control = c,
            Apply = events => Replay(c, events),
        };
    }

    /// The schema's fields of a canvas: its size in the window's pixels, and no input yet.
    internal static void Write(Utf8JsonWriter jw, DialogSettings.Setting s)
    {
        jw.WriteNumber("width", s.Control?.Width ?? 0);
        jw.WriteNumber("height", s.Control?.Height ?? 0);
        jw.WriteStartArray("default");
        jw.WriteEndArray();
    }

    private static IEnumerable<Control> Tree(Control c)
    {
        yield return c;
        foreach (Control child in c.Controls)
            foreach (Control d in Tree(child)) yield return d;
    }

    private static bool Draws(Control c)
        => Own(c, "OnPaint", typeof(PaintEventArgs)) || Own(c, "OnPaintBackground", typeof(PaintEventArgs)) || Handled(c, "Paint")
           || GdiPaint(c) is { } gdi && Own(c, "OnGdiPaint", gdi.GetParameters()[0].ParameterType)
           || IsDirect2DControl(c);

    /// A Direct2DControl subclass paints through its OnRender, which the control's OnPaint runs. The plug-in overrides
    /// OnRender, not OnPaint, so it is not caught by Own above; detect it by its base type instead.
    private static bool IsDirect2DControl(Control c)
    {
        for (Type? t = c.GetType(); t is not null; t = t.BaseType)
            if (t.FullName == "PaintDotNet.Controls.Direct2DControl") return true;
        return false;
    }

    /// A Paint.NET GDI-painted control's paint (`GdiPaintControl.DoGdiPaint`, which runs its OnGdiPaint), or null for
    /// a control that paints through OnPaint. CurtisBlack's document view paints so.
    private static MethodInfo? GdiPaint(Control c)
    {
        for (Type? t = c.GetType(); t is not null; t = t.BaseType)
            if (t.FullName == "PaintDotNet.SystemLayer.GdiPaintControl") return t.GetMethod("DoGdiPaint");
        return null;
    }

    private static bool TakesPointer(Control c)
        => new[] { "OnMouseDown", "OnMouseMove", "OnMouseUp" }.Any(m => Own(c, m, typeof(MouseEventArgs)))
           || new[] { "MouseDown", "MouseMove", "MouseUp" }.Any(e => Handled(c, e));

    private static bool TakesWheel(Control c) => Own(c, "OnMouseWheel", typeof(MouseEventArgs)) || Handled(c, "MouseWheel");

    /// Whether the plug-in's own classes override Control's `name` (taking `arg`).
    private static bool Own(Control c, string name, Type arg)
    {
        MethodInfo? m = c.GetType().GetMethod(name, Instance, null, [arg], null);
        string? asm = m?.DeclaringType?.Assembly.GetName().Name;
        return asm is not null && asm != "System.Windows.Forms" && !asm.StartsWith("PaintDotNet", StringComparison.Ordinal);
    }

    /// Whether Control's event `name` has a handler.
    private static bool Handled(Control c, string name)
    {
        for (Type? t = typeof(Control); t is not null; t = t.BaseType)
            if (t.GetField(name, Instance | BindingFlags.DeclaredOnly) is { } f)
                return f.GetValue(c) is Delegate;
        return false;
    }

    private static void Call(Control c, string name, EventArgs e)
        => Invoke(typeof(Control).GetMethod(name, Instance, null, [e.GetType()], null), c, e);

    /// `m` on `c`, a plug-in's own exception rethrown as itself rather than wrapped by the reflection call.
    private static void Invoke(MethodInfo? m, object c, object arg)
    {
        try { m?.Invoke(c, [arg]); }
        catch (TargetInvocationException e) when (e.InnerException is { } inner)
        {
            System.Runtime.ExceptionServices.ExceptionDispatchInfo.Capture(inner).Throw();
        }
    }

    /// Where the pointer is in a canvas: the control it is over, the one a press holds it to, and the buttons down.
    private sealed class Route
    {
        public Control? Hovered, Captured;
        public MouseButtons Held;
    }

    private static readonly ConditionalWeakTable<Control, Route> Routes = new();

    /// The deepest visible control at (x, y) of `c`'s client area, and the point in its own.
    private static (Control Target, int X, int Y) Hit(Control c, int x, int y)
    {
        for (int i = 0; i < c.Controls.Count; i++)   // the first is on top, as in Windows
        {
            Control child = c.Controls[i];
            if (child.Visible && child.Bounds.Contains(x, y)) return Hit(child, x - child.Left, y - child.Top);
        }
        return (c, x, y);
    }

    /// (x, y) of `root`'s client area in `c`'s, for `c` in `root` (or root itself); a control around the canvas (its
    /// form) gets the canvas's own point.
    private static Point Into(Control root, Control c, int x, int y)
    {
        if (!Tree(root).Contains(c)) return new Point(x, y);
        for (Control? p = c; p is not null && p != root; p = p.Parent) (x, y) = (x - p.Left, y - p.Top);
        return new Point(x, y);
    }

    /// Feeds recorded input into the canvas `c`, one event at a time, as Windows would deliver it:
    /// {"e": "down"|"move"|"up"|"wheel"|"leave"|"key"|"keyup"|"char"|"tick", "x", "y" (the canvas's pixels),
    /// "b": "left"|"right"|"middle", "d" (wheel delta), "k" (a Keys value, modifiers included), "c" (a typed character).
    /// "tick" is "ms" milliseconds of the window's running timers (a brush that grows while held): each enabled one
    /// ticks once per whole Interval elapsed, the rest carried to the next tick.
    public static void Replay(Control c, JsonElement events)
    {
        if (events.ValueKind != JsonValueKind.Array) return;
        // A code editor (CodeLab's script box) takes its own events, its text and selection as paint.rs edits them.
        if (EditControl?.Invoke(c, events) == true)
        {
            RunQueued?.Invoke();
            return;
        }
        Route route = Routes.GetOrCreateValue(c);
        foreach (JsonElement ev in events.EnumerateArray())
        {
            string kind = Text(ev, "e");
            int x = Number(ev, "x"), y = Number(ev, "y");
            MouseButtons button = Text(ev, "b") switch
            {
                "right" => MouseButtons.Right,
                "middle" => MouseButtons.Middle,
                "left" => MouseButtons.Left,
                _ => MouseButtons.None,
            };
            switch (kind)
            {
                case "down" or "move" or "up":
                {
                    Control target;
                    Point at;
                    if (route.Captured is { } held) (target, at) = (held, Into(c, held, x, y));
                    else
                    {
                        (Control hit, int hx, int hy) = Hit(c, x, y);
                        (target, at) = (hit, new Point(hx, hy));
                        if (hit != route.Hovered)
                        {
                            if (route.Hovered is { } old) Call(old, "OnMouseLeave", EventArgs.Empty);
                            Call(hit, "OnMouseEnter", EventArgs.Empty);
                            route.Hovered = hit;
                        }
                    }
                    if (kind == "down")
                    {
                        // Windows paints what the last input invalidated before the user presses.
                        if (Tree(c).Any(t => t.HostPaintDue)) Paint(c);
                        route.Captured = target;
                        route.Held |= button;
                        Call(target, "OnMouseDown", new MouseEventArgs(button, 1, at.X, at.Y, 0));
                    }
                    else if (kind == "move")
                        Call(target, "OnMouseMove", new MouseEventArgs(route.Held, 0, at.X, at.Y, 0));
                    else
                    {
                        route.Held &= ~button;
                        if (route.Held == MouseButtons.None) route.Captured = null;
                        Call(target, "OnMouseUp", new MouseEventArgs(button, 1, at.X, at.Y, 0));
                        Call(target, "OnMouseClick", new MouseEventArgs(button, 1, at.X, at.Y, 0));
                        Call(target, "OnClick", EventArgs.Empty);
                    }
                    break;
                }
                case "leave":
                    if (route.Captured is null && route.Hovered is { } gone)
                    {
                        Call(gone, "OnMouseLeave", EventArgs.Empty);
                        route.Hovered = null;
                    }
                    break;
                case "wheel":
                {
                    // To the control under the pointer, else the nearest around it that takes the wheel (the form too).
                    Control? at = Hit(c, x, y).Target;
                    while (at is not null && !TakesWheel(at)) at = at.Parent;
                    if (at is not null)
                    {
                        Point p = Into(c, at, x, y);
                        Call(at, "OnMouseWheel", new MouseEventArgs(MouseButtons.None, 0, p.X, p.Y, Number(ev, "d")));
                    }
                    break;
                }
                case "key" or "keyup" or "char":
                    foreach (Control to in KeyTargets(c))
                    {
                        if (kind == "char")
                        {
                            string ch = Text(ev, "c");
                            if (ch.Length == 0) break;
                            var e = new KeyPressEventArgs(ch[0]);
                            Call(to, "OnKeyPress", e);
                            if (e.Handled) break;
                        }
                        else
                        {
                            var e = new KeyEventArgs((Keys)Number(ev, "k"));
                            Call(to, kind == "key" ? "OnKeyDown" : "OnKeyUp", e);
                            if (e.Handled) break;
                        }
                    }
                    break;
                case "tick": Tick(c, Number(ev, "ms")); break;
            }
            RunQueued?.Invoke();
        }
    }

    /// Who gets a key: the form first when it previews keys (KeyPreview), then the canvas, which has the focus.
    private static IEnumerable<Control> KeyTargets(Control c)
    {
        if (c.FindForm() is { KeyPreview: true } form && form != c) yield return form;
        yield return c;
    }

    private static string Text(JsonElement e, string name)
        => e.TryGetProperty(name, out JsonElement v) && v.ValueKind == JsonValueKind.String ? v.GetString()! : "";

    private static int Number(JsonElement e, string name)
        => e.TryGetProperty(name, out JsonElement v) && v.ValueKind == JsonValueKind.Number ? (int)v.GetDouble() : 0;

    /// The milliseconds a timer has run since it last ticked.
    private sealed class Run { public int Ms; public long Last; public int Took = -1; }

    private static readonly ConditionalWeakTable<System.Windows.Forms.Timer, Run> Elapsed = new();

    /// `ms` of every enabled Windows Forms timer the window holds (in a component container, the designer's
    /// `components`, or a field of the form or of one of its controls): its Tick once per whole Interval.
    private static void Tick(Control c, int ms)
    {
        foreach (System.Windows.Forms.Timer t in Timers(c).Where(t => t.Enabled))
        {
            Run run = Elapsed.GetOrCreateValue(t);
            run.Ms += Math.Max(0, ms);
            for (int interval = Math.Max(1, t.Interval); run.Ms >= interval && t.Enabled; run.Ms -= interval)
                Fire(t);
        }
    }

    /// What a message loop does for a window between the user's actions, done when the host has loaded one, replayed
    /// input into it or pressed one of its buttons: the work it queued runs, and each timer enabled then ticks once
    /// (a delayed rebuild fires, an animation steps once). A tick may start another timer: that one ticks in the next
    /// round, up to a few rounds; no timer ticks twice in one settle. True when one ticked. With `due`, only the timers
    /// whose Interval has passed since they started or last ticked tick, as they would by now (a rebuild delayed
    /// until the user stops typing).
    public static bool Settle(Control c, bool due = false)
    {
        // Its Scintilla editors found first, so each is told of its document's changes from here on.
        Control root = c;
        while (root.Parent is { } parent) root = parent;
        foreach (Control control in Tree(root)) ScintillaBox.Of(control);
        RunQueued?.Invoke();
        var ticked = new HashSet<System.Windows.Forms.Timer>();
        for (int round = 0; round < 4; round++)
        {
            List<System.Windows.Forms.Timer> firing = Timers(c).Where(t => t.Enabled && (!due || DueIn(t) == 0) && ticked.Add(t)).ToList();
            if (firing.Count == 0) break;
            foreach (System.Windows.Forms.Timer t in firing)
                if (t.Enabled)
                {
                    Fire(t);
                    Elapsed.GetOrCreateValue(t).Last = Environment.TickCount64;
                }
        }
        return ticked.Count > 0;
    }

    /// The milliseconds until the first of `c`'s window's enabled timers is due, or -1 when none runs.
    public static int DueIn(Control c)
        => Timers(c).Where(t => t.Enabled).Select(DueIn).DefaultIfEmpty(-1).Min();

    private static int DueIn(System.Windows.Forms.Timer t)
    {
        long since = Math.Max(t.HostStarted, Elapsed.TryGetValue(t, out Run? run) ? run.Last : 0);
        return (int)Math.Clamp(Math.Max(1, t.Interval) - (Environment.TickCount64 - since), 0, int.MaxValue);
    }

    private static readonly FieldInfo? TickEvent = typeof(System.Windows.Forms.Timer).GetField("Tick", Instance);

    // One tick. A tick that throws ends there and the window carries on, as Windows Forms reports such an exception
    // (Application.ThreadException) rather than ending the user's action with it.
    private static void Fire(System.Windows.Forms.Timer t)
    {
        long start = Environment.TickCount64;
        try
        {
            (TickEvent?.GetValue(t) as EventHandler)?.Invoke(t, EventArgs.Empty);
            RunQueued?.Invoke();
        }
        catch (Exception ex) when (ex is not OutOfMemoryException)
        {
            if (PluginHost.Trace) Console.Error.WriteLine($"timer tick: {ex}");
        }
        Elapsed.GetOrCreateValue(t).Took = (int)Math.Min(int.MaxValue, Environment.TickCount64 - start);
    }

    /// Whether a timer due within `ms` of now may keep the window busy: one that has not ticked yet, or whose last
    /// tick took longer than a frame or so (a rebuild). paint.rs says "Working…" before such a settle.
    public static bool SlowSoon(Control c, int ms)
        => Timers(c).Any(t => t.Enabled && DueIn(t) <= ms && (!Elapsed.TryGetValue(t, out Run? run) || run.Took < 0 || run.Took > 100));

    /// The Windows Forms timers `c`'s window holds: in a component container (the designer's `components`), or in a
    /// field of the form or of one of its controls.
    private static HashSet<System.Windows.Forms.Timer> Timers(Control c)
    {
        Control root = c;
        while (root.Parent is { } parent) root = parent;
        var seen = new HashSet<System.Windows.Forms.Timer>();
        foreach (Control control in Tree(root))
            foreach (FieldInfo f in control.GetType().GetFields(Instance))
                switch (f.GetValue(control))
                {
                    case System.Windows.Forms.Timer t: seen.Add(t); break;
                    case IContainer box:
                        foreach (object part in box.Components)
                            if (part is System.Windows.Forms.Timer t2) seen.Add(t2);
                        break;
                }
        return seen;
    }

    /// What the canvas draws now, straight BGRA8 at its size: it, then the controls in it at their places (the
    /// first on top), each its background then its OnPaint.
    public static (byte[] Bgra, int Width, int Height) Paint(Control c)
    {
        Refresh(c);
        int w = Math.Max(1, c.Width), h = Math.Max(1, c.Height);
        using var bitmap = new Bitmap(w, h, PixelFormat.Format32bppArgb);
        using (Graphics g = Graphics.FromImage(bitmap)) Draw(g, c);
        BitmapData data = bitmap.LockBits(new Rectangle(0, 0, w, h), ImageLockMode.ReadOnly, PixelFormat.Format32bppArgb);
        try
        {
            var bgra = new byte[w * h * 4];
            for (int row = 0; row < h; row++)
                Marshal.Copy(data.Scan0 + row * data.Stride, bgra, row * w * 4, w * 4);
            return (bgra, w, h);
        }
        finally { bitmap.UnlockBits(data); }
    }

    /// The window's surfaces into the bitmaps that alias them (Liquify shows its warped image through one): the stand-in
    /// Surface's GDI view is a copy, not an alias, so what the plug-in rendered into a surface since is pushed to it
    /// first. Every Surface held in a field of the form or of a control in it.
    private static void Refresh(Control c)
    {
        Control root = c;
        while (root.Parent is { } parent) root = parent;
        var seen = new HashSet<PaintDotNet.Surface>(ReferenceEqualityComparer.Instance);
        foreach (Control control in Tree(root))
            foreach (FieldInfo f in control.GetType().GetFields(Instance))
                if (f.FieldType == typeof(PaintDotNet.Surface) && f.GetValue(control) is PaintDotNet.Surface s && seen.Add(s))
                    s.PushToGdi();
    }

    private static void Draw(Graphics g, Control c)
    {
        c.HostPaintDue = false;
        var area = new Rectangle(0, 0, c.Width, c.Height);
        GraphicsState state = g.Save();
        g.SetClip(area, CombineMode.Intersect);
        if (GdiPaint(c) is { } gdi)
        {
            // Its device context is the bitmap's Graphics, here at the control's place: the host's
            // PdnGraphics.DrawBitmap draws through it.
            GCHandle hdc = GCHandle.Alloc(g);
            try
            {
                Type type = gdi.GetParameters()[0].ParameterType;
                object context = Activator.CreateInstance(type)!;
                type.GetProperty("Hdc")!.SetValue(context, GCHandle.ToIntPtr(hdc));
                type.GetProperty("UpdateRect")!.SetValue(context, area);
                type.GetProperty("UpdateRegion")!.SetValue(context, new[] { area });
                Invoke(gdi, c, context);
            }
            finally { hdc.Free(); }
        }
        else
        {
            using var e = new PaintEventArgs(g, area);
            Call(c, "OnPaintBackground", e);
            Call(c, "OnPaint", e);
        }
        for (int i = c.Controls.Count - 1; i >= 0; i--)
        {
            Control child = c.Controls[i];
            if (!child.Visible || child.Width <= 0 || child.Height <= 0) continue;
            GraphicsState inner = g.Save();
            g.TranslateTransform(child.Left, child.Top);
            Draw(g, child);
            g.Restore(inner);
        }
        g.Restore(state);
    }
}
