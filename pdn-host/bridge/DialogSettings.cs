// The settings a plug-in's own Windows Forms settings window offers, as paint.rs settings. Each value control in
// the window is one setting, keyed by its place in the window: track bars and numeric boxes (ranges), check boxes,
// radio-button groups, drop-downs and lists (choices), text boxes, tool-strip drop-downs and toggles, and controls
// that pick a colour (a swatch that opens a colour dialog, or a plug-in's own colour or value control). The schema
// lists them; a render loads a fresh window and replays the user's values into it as a user's input would arrive
// (a drag, a click, a pick), so the plug-in's own handlers make the token it renders with. A custom-drawn preview
// is no setting: the values beside it are what the user adjusts. The window's buttons (one that adds a line to a
// list, resets, randomizes) are pressed only when the user presses them (PluginHost.Press), and the token that
// makes is kept (TokenJson), as Paint.NET keeps a window's token, rather than replayed.
using System;
using System.Collections.Generic;
using System.Drawing;
using System.Globalization;
using System.Linq;
using System.Reflection;
using System.Text.Json;
using System.Text.RegularExpressions;
using System.Threading;
using System.Windows.Forms;
using PaintDotNet.Effects;

namespace PaintDotNet.Bridge;

internal static class DialogSettings
{
    /// One adjustable value. `Apply` sets it from a render's props (a number, a bool, a choice index, a string, or a
    /// colour as "#rrggbb", "primary" or "secondary"); `Read` gives a numeric one's current value.
    internal sealed class Setting
    {
        public required string Key, Type, Label;
        public double Min, Max, Default, Step = 1;
        public string Text = "";
        public string[] Choices = [];
        public Color Color;
        public required Action<JsonElement> Apply;
        /// The value is what the window shows now (null: cannot tell, as for a colour swatch).
        public Func<JsonElement, bool>? Same;
        public Func<double>? Read;
        public bool IsTrackBar;
        /// Its label is a caption the window shows (not made from the control's name).
        public bool Named;
        /// Where its control sits in the window, to number settings that end up with the same name.
        public Point At;
        /// The plug-in control a setting came from, when the paint.rs side needs it (a canvas reads its size).
        public Control? Control;
    }

    /// The window's settings as paint.rs shows them, from a window `load` builds and loads: one window to find the
    /// controls that move together (a track bar and the numeric box beside it), which then show once, and a fresh
    /// one for the defaults.
    public static List<Setting> Of(Func<Control> load)
    {
        Control scratch = load();
        Dictionary<string, string> linked = Linked(Collect(scratch, false));
        (scratch as IDisposable)?.Dispose();
        Control fresh = load();
        List<Setting> all = Collect(fresh, true);
        (fresh as IDisposable)?.Dispose();
        // The one of a pair that stays takes its partner's caption if it has none (the label sits by the track bar).
        var byKey = all.ToDictionary(s => s.Key);
        foreach ((string gone, string kept) in linked)
            if (byKey.TryGetValue(gone, out Setting? g) && byKey.TryGetValue(kept, out Setting? k) && g.Named && !k.Named)
                (k.Label, k.Named) = (g.Label, true);
        List<Setting> shown = all.Where(s => !linked.ContainsKey(s.Key)).ToList();
        return Finish(shown);
    }

    /// The settings of a window shown now (a FileType's, while it opens or saves), with the values it shows: as Of,
    /// read from that one window, which is not loaded again, so a track bar and the numeric box beside it both show.
    public static List<Setting> OfShown(Control root) => Finish(Collect(root, true));

    private static List<Setting> Finish(List<Setting> shown)
    {
        // Settings still sharing a name (a table's rows under one column header, swatches in one box) are numbered
        // in the window's reading order.
        foreach (var same in shown.GroupBy(s => s.Label).Where(g => g.Count() > 1))
        {
            int n = 1;
            foreach (Setting each in same.OrderBy(s => s.At.Y).ThenBy(s => s.At.X))
                each.Label = $"{each.Label} {n++}";
        }
        return shown;
    }

    /// Sets the settings `props` gives into the freshly loaded window `root`: only those that differ from what the
    /// window shows when loaded, drop-downs and radio groups first (one may be a preset that sets the others, which
    /// the user's own changes then refine), then the rest in the window's order.
    public static void Apply(Control root, JsonElement props)
    {
        var changed = new List<(Setting, JsonElement)>();
        foreach (Setting s in Collect(root, false))
            if (s.Type != "button" && props.TryGetProperty(s.Key, out JsonElement v) && !IsSame(s, v))
                changed.Add((s, v));
        foreach ((Setting s, JsonElement v) in changed.Where(c => c.Item1.Type == "choice").Concat(changed.Where(c => c.Item1.Type != "choice")))
            try { s.Apply(v); }
            catch (Exception) { }   // a value the window refuses leaves it as it was
    }

    // Whether `v` is what the setting shows already. A value it cannot read is not the same: Apply then tries it and
    // leaves the setting as it was, and the settings after it are still applied.
    private static bool IsSame(Setting s, JsonElement v)
    {
        try { return s.Same?.Invoke(v) == true; }
        catch (Exception) { return false; }
    }

    // A whole number paint.rs sent, as a number or as its digits (a choice row the window gained after it opened
    // comes back as a string).
    private static int Whole(JsonElement v)
        => v.ValueKind == JsonValueKind.String ? int.Parse(v.GetString()!, System.Globalization.CultureInfo.InvariantCulture) : v.GetInt32();

    /// Presses the button `key` of the loaded window `root`; false when it has none.
    public static bool Press(Control root, string key)
    {
        if (Collect(root, false).FirstOrDefault(s => s.Key == key && s.Type == "button") is not { } button) return false;
        button.Apply(default);
        return true;
    }

    /// The loaded window's settings with the values it shows now (as their defaults).
    public static List<Setting> Current(Control root) => Collect(root, false);

    /// The loaded window's current settings shaped exactly as the open-time schema (Of): each slider/spinner pair
    /// collapsed to the one kept key (taking its partner's label), then repeated labels numbered. So paint.rs, re-
    /// reading a live window, sees the same keys it got at open plus only the rows the window genuinely gained -- not
    /// the partner keys Of had linked away (which raw Current still lists).
    public static List<Setting> CurrentShaped(Control root)
    {
        List<Setting> all = Collect(root, false);
        Dictionary<string, string> linked = Linked(all);
        var byKey = all.ToDictionary(s => s.Key);
        foreach ((string gone, string kept) in linked)
            if (byKey.TryGetValue(gone, out Setting? g) && byKey.TryGetValue(kept, out Setting? k) && g.Named && !k.Named)
                (k.Label, k.Named) = (g.Label, true);
        List<Setting> shown = all.Where(s => !linked.ContainsKey(s.Key)).ToList();
        return Finish(shown);
    }

    /// The schema's properties for `settings` (docs/pdn-bridge.md "effect schema").
    public static void WriteProperties(Utf8JsonWriter jw, List<Setting> settings)
    {
        jw.WriteStartArray("properties");
        foreach (Setting s in settings)
        {
            jw.WriteStartObject();
            jw.WriteString("name", s.Key);
            jw.WriteString(s.Type == "bool" ? "description" : "label", s.Label);
            jw.WriteString("type", s.Type);
            switch (s.Type)
            {
                case "int" or "double":
                    jw.WriteNumber("default", s.Default);
                    jw.WriteNumber("min", s.Min);
                    jw.WriteNumber("max", s.Max);
                    if (s.Type == "double") jw.WriteNumber("step", s.Step);
                    break;
                case "bool": jw.WriteBoolean("default", s.Default != 0); break;
                case "string": jw.WriteString("default", s.Text); jw.WriteNumber("maxLength", s.Max); break;
                case "color": jw.WriteNumber("default", s.Color.ToArgb() & 0xFFFFFF); break;
                case "canvas": DialogCanvas.Write(jw, s); break;
                case "code": ScintillaBox.Write(jw, s); break;
                case "file": jw.WriteString("filter", s.Text); break;
                case "list":
                    jw.WriteNumber("default", s.Default);
                    jw.WriteStartArray("items");
                    foreach (string item in s.Choices) jw.WriteStringValue(item);
                    jw.WriteEndArray();
                    break;
                case "choice":
                    jw.WriteNumber("default", s.Default);
                    jw.WriteStartArray("choices");
                    for (int i = 0; i < s.Choices.Length; i++)
                    {
                        jw.WriteStartObject();
                        jw.WriteNumber("value", i);
                        jw.WriteString("label", s.Choices[i]);
                        jw.WriteEndObject();
                    }
                    jw.WriteEndArray();
                    break;
            }
            jw.WriteEndObject();
        }
        jw.WriteEndArray();
    }

    /// The control at `key` ("w:" then each child's index) under `root`.
    private static Control? Resolve(Control root, string key)
    {
        Control c = root;
        foreach (string i in key["w:".Length..].Split('.', StringSplitOptions.RemoveEmptyEntries))
        {
            if (!int.TryParse(i, NumberStyles.None, CultureInfo.InvariantCulture, out int n) || n >= c.Controls.Count) return null;
            c = c.Controls[n];
        }
        return c;
    }

    /// Numeric settings that follow another's value, each to the one that stays: each in turn is moved, and any
    /// other beside it (in the same container) that moves with it is linked to it. Of a pair the numeric box stays
    /// (it carries the decimals), else the first.
    private static Dictionary<string, string> Linked(List<Setting> settings)
    {
        var numeric = settings.Where(s => s.Read is not null).ToList();
        var linked = new Dictionary<string, string>();
        foreach (Setting a in numeric)
        {
            if (linked.ContainsKey(a.Key)) continue;
            var before = numeric.ToDictionary(s => s.Key, s => s.Read!());
            double to = before[a.Key] != a.Min ? a.Min : a.Max;
            try { a.Apply(JsonSerializer.SerializeToElement(to)); } catch (Exception) { continue; }
            foreach (Setting b in numeric)
                if (b != a && !linked.ContainsKey(b.Key) && Container(b.Key) == Container(a.Key) && b.Read!() != before[b.Key])
                {
                    if (a.IsTrackBar && !b.IsTrackBar) linked.TryAdd(a.Key, b.Key);
                    else linked.TryAdd(b.Key, a.Key);
                }
        }
        return linked;
    }

    /// The container of the control a key names: "w:0.3." (a control), "w:0.3.value" (its own value) and "w:0.3.5"
    /// (a tool-strip item) all sit in "w:0.".
    private static string Container(string key)
    {
        string control = key.EndsWith('.') ? key[..^1] : key[..key.LastIndexOf('.')];
        return control[..(control.LastIndexOfAny(['.', ':']) + 1)];
    }

    /// The window's settings in its order. `probe`: find each colour swatch's colour by clicking it with its colour
    /// dialog left unanswered (only while reading the schema; a render never needs the defaults).
    private static List<Setting> Collect(Control root, bool probe)
    {
        var list = new List<Setting>();
        Walk(root, "w:", list, probe);
        return list;
    }

    private static void Walk(Control c, string path, List<Setting> list, bool probe)
    {
        if (!c.Visible && c is not TabPage) return;   // a tab's page is hidden until chosen, its settings are not
        string? caption = Caption(c);
        int first = list.Count;
        bool leaf = Own(c, path, caption ?? (Plain(ToolTip.HostTipOf(c)) is { Length: > 0 } tip ? tip : Humanize(c.Name)), list, probe);
        Point at = Point.Empty;
        for (Control? p = c; p is not null; p = p.Parent) at.Offset(p.Left, p.Top);
        for (int k = first; k < list.Count; k++) (list[k].Named, list[k].At) = (list[k].Named || caption is not null, at);
        // With PDN_BRIDGE_TRACE set, the window's layout as read: each setting's control and the caption it took, and
        // each caption control, to see where a setting's name came from.
        if (PluginHost.Trace)
        {
            for (int k = first; k < list.Count; k++)
                Console.Error.WriteLine($"dialog settings: {list[k].Key} {list[k].Type} \"{list[k].Label}\" <- {c.GetType().Name} {c.Name} {c.Bounds} text \"{Plain(c.Text)}\" tip \"{Plain(ToolTip.HostTipOf(c))}\"");
            if (IsCaption(c)) Console.Error.WriteLine($"dialog settings: caption \"{Plain(c.Text)}\" {c.GetType().Name} {c.Name} {c.Bounds}");
        }
        if (leaf) return;
        var radios = new List<RadioButton>();
        for (int i = 0; i < c.Controls.Count; i++)
        {
            Control child = c.Controls[i];
            if (child is RadioButton { AutoCheck: true, Visible: true } r) radios.Add(r);
            else Walk(child, $"{path}{i}.", list, probe);
        }
        if (radios.Count > 1)
        {
            // The group's box, else a caption by its first button, else one by the panel holding the buttons.
            string? group = c is GroupBox g && Plain(g.Text) is { Length: > 0 } gt ? gt : Caption(radios[0]) ?? Caption(c);
            Setting choice = Choice(path + "radio", group ?? Humanize(radios[0].Name), radios.Select(Shown).ToArray(),
                                    () => radios.FindIndex(r => r.Checked), i => Click(radios[i]));
            choice.Named = group is not null;
            list.Add(choice);
        }
    }

    /// The settings of `c` itself; true when it is a value control whose children are none of the window's.
    private static bool Own(Control c, string path, string label, List<Setting> list, bool probe)
    {
        // A Scintilla editor (CodeLab's script box): its text as paint.rs's code editor shows it.
        if (ScintillaBox.Setting(c, path, label) is { } code)
        {
            list.Add(code);
            return true;
        }
        switch (c)
        {
            case TrackBar tb:
                list.Add(Range(path, label, tb.Minimum, tb.Maximum, tb.Value, 0, 1, v => tb.ScrollTo((int)Math.Round(v)), () => tb.Value, true));
                return true;
            case NumericUpDown nud:
                list.Add(Range(path, label, (double)nud.Minimum, (double)nud.Maximum, (double)nud.Value, nud.DecimalPlaces, (double)nud.Increment,
                               v => nud.Value = Math.Clamp((decimal)v, nud.Minimum, nud.Maximum), () => (double)nud.Value, false));
                return true;
            case DomainUpDown dud when dud.Items.Count > 0:
                list.Add(Choice(path, label, Texts(dud.Items, i => i?.ToString() ?? ""), () => dud.SelectedIndex, i => dud.SelectedIndex = i));
                return true;
            case UpDownBase:
                return true;   // its own arrow buttons and edit box are no settings
            case CheckBox cb:
                list.Add(new Setting
                {
                    Key = path, Type = "bool", Label = Plain(cb.Text) is { Length: > 0 } t ? t : label, Default = cb.Checked ? 1 : 0,
                    Named = Plain(cb.Text).Length > 0,
                    Apply = v => { if (cb.Checked != v.GetBoolean()) Click(cb); },
                    Same = v => cb.Checked == v.GetBoolean(),
                });
                return true;
            case ComboBox cbx when cbx.Items.Count > 0:
                list.Add(Choice(path, label, Texts(cbx.Items, cbx.GetItemText), () => cbx.SelectedIndex, i => cbx.Pick(i)));
                return true;
            case ListBox lb when lb.SelectionMode == SelectionMode.One:
                // A list shows its items as they are now (buttons may add to it), and which one is selected.
                list.Add(new Setting
                {
                    Key = path, Type = "list", Label = label, Choices = Texts(lb.Items, i => Describe(lb, i)), Default = lb.SelectedIndex,
                    Apply = v => { int i = Whole(v); if (i >= -1 && i < lb.Items.Count) lb.SelectedIndex = i; },
                    Same = v => Whole(v) == lb.SelectedIndex,
                });
                return true;
            case TextBoxBase tx when !tx.ReadOnly:
                list.Add(new Setting
                {
                    Key = path, Type = "string", Label = label, Text = tx.Text, Max = tx.MaxLength,
                    Apply = v => tx.Text = v.GetString() ?? "", Same = v => tx.Text == v.GetString(),
                });
                return true;
            case ToolStrip ts:
                ToolStripItems(ts.Items, path, list, probe);
                return true;
        }
        // A swatch shows its colour, not a word: a button with text ("Reset All") is a command even when it sets one.
        if (Plain(c.Text).Length == 0 && PicksColour(c) is { } handlers)
        {
            list.Add(ColourPicker(path, label, c, handlers, probe));
            return false;
        }
        // A button that opens a file chooser (ScriptLab loads/saves a script, a preset loader): paint.rs offers
        // its own file picker and replays the chosen file into the plug-in's OpenFileDialog, rather than the
        // button being dropped as unanswerable.
        if (c is Button fb && fb.DialogResult == DialogResult.None && OpensFile(fb) is { } fileHandlers)
        {
            list.Add(FilePicker(path, label, fb, fileHandlers, probe));
            return true;
        }
        // A button that does something in the window: not the window's own OK and Cancel, nor a swatch (above).
        if (c is Button b && b.DialogResult == DialogResult.None && b.FindForm() is var form
            && (form is null || (form.AcceptButton != b && form.CancelButton != b)) && Pressable(b))
        {
            // Its text names it, else its tip; a one-letter one ("X") is better named by the control's name.
            string name = Plain(b.Text) is { Length: > 1 } text ? text : Plain(ToolTip.HostTipOf(b)) is { Length: > 0 } tip ? tip
                : Humanize(b.Name) is { Length: > 0 } named ? named : Plain(b.Text);
            list.Add(new Setting { Key = path, Type = "button", Label = name, Named = true, Apply = _ => Click(b) });
            return true;
        }
        // A plug-in container that holds plug-in child controls of its own (CurtisBlack's ColorPicker: a colour
        // wheel, R/G/B/H/S/V/A sliders, a swatch, buttons) is not one value or one canvas. Descend into it so each
        // child maps on its own, as the user sees them in Paint.NET; only a leaf (no plug-in children) collapses to
        // its Color/Value or becomes a canvas.
        bool container = HasPluginChildren(c);
        if (c is not Form && !container && IsPluginControl(c) && OwnValue(c, path, label) is { } own)
        {
            list.Add(own);
            return true;
        }
        // A custom-drawn control that is none of the above (Liquify's warp brush): paint.rs shows its OnPaint
        // and replays the user's input events into it (DialogCanvas, dj8's live canvas).
        if (c is not Form && !container && IsPluginControl(c) && DialogCanvas.Canvas(c, path, label) is { } canvas)
        {
            list.Add(canvas);
            return true;
        }
        // A plug-in container whose children ALL map to canvases (CurtisBlack's DocumentView: a GdiPaintControl
        // shadow, the document box and the rulers -- one pan/zoom view, not four stacked) is itself one canvas.
        // Collect the children's settings for their types only (probe off, so no swatch side effects); if every one
        // is a canvas, collapse to the container's own canvas instead of descending. A value child (ColorPicker's
        // sliders) leaves a non-canvas in the scratch, so it descends as before.
        if (container && c is not Form && IsPluginControl(c))
        {
            var scratch = new List<Setting>();
            foreach (Control child in c.Controls.Cast<Control>()) Walk(child, path, scratch, false);
            if (scratch.Count > 0 && scratch.TrueForAll(s => s.Type == "canvas") && DialogCanvas.Canvas(c, path, label) is { } one)
            {
                list.Add(one);
                return true;
            }
        }
        return false;
    }

    /// Whether `c` holds plug-in child controls of its own (so it is a container to descend into, not a leaf value).
    private static bool HasPluginChildren(Control c) => c.Controls.Cast<Control>().Any(IsPluginControl);

    private static void ToolStripItems(ToolStripItemCollection items, string path, List<Setting> list, bool probe)
    {
        string? caption = null;   // a tool-strip label names the items after it
        for (int i = 0; i < items.Count; i++)
        {
            ToolStripItem item = items[i];
            string key = $"{path}{i}";
            string label = Plain(item.ToolTipText) is { Length: > 0 } tip ? tip
                         : caption ?? (item is not ToolStripControlHost && Plain(item.Text) is { Length: > 0 } tx ? tx : Humanize(item.Name));
            switch (item)
            {
                case ToolStripLabel l: caption = Plain(l.Text) is { Length: > 0 } lt ? lt.TrimEnd(':') : null; continue;
                case ToolStripComboBox tc when tc.Items.Count > 0:
                    list.Add(Choice(key, label, Texts(tc.Items, tc.ComboBox.GetItemText), () => tc.SelectedIndex, j => tc.ComboBox.Pick(j)));
                    break;
                case ToolStripTextBox tt:
                    list.Add(new Setting { Key = key, Type = "string", Label = label, Text = tt.Text, Apply = v => tt.Text = v.GetString() ?? "", Same = v => tt.Text == v.GetString() });
                    break;
                case ToolStripButton { CheckOnClick: true, Visible: true } b:
                    list.Add(new Setting
                    {
                        Key = key, Type = "bool", Label = Plain(b.Text) is { Length: > 0 } bt ? bt : label, Default = b.Checked ? 1 : 0,
                        Apply = v => { if (b.Checked != v.GetBoolean()) b.PerformClick(); },
                        Same = v => b.Checked == v.GetBoolean(),
                    });
                    break;
                case ToolStripControlHost host:
                    int first = list.Count;
                    Walk(host.Control, key + ".", list, probe);
                    if (caption is not null)   // a hosted control is named by the tool-strip label before it
                        for (int k = first; k < list.Count; k++)
                            if (!list[k].Named) (list[k].Label, list[k].Named) = (caption, true);
                    break;
            }
            caption = null;
        }
    }

    private static Setting Range(string key, string label, double min, double max, double value, int decimals, double step,
                                 Action<double> set, Func<double> read, bool trackBar)
        => new()
        {
            Key = key, Type = decimals > 0 ? "double" : "int", Label = label, Min = min, Max = max, Default = value,
            Step = decimals > 0 ? Math.Max(step, Math.Pow(10, -decimals)) : 1,
            Apply = v => { double to = Math.Clamp(v.GetDouble(), min, max); if (to != read()) set(to); }, Read = read, IsTrackBar = trackBar,
            Same = v => Math.Abs(Math.Clamp(v.GetDouble(), min, max) - read()) < 1e-9,
        };

    /// A drop-down, list or radio group. Items that end in "..." are commands (open a dialog: "Manage presets...",
    /// "Add/Remove Brushes...") and are left out; the setting's value is the position among the rest.
    private static Setting Choice(string key, string label, string[] items, Func<int> selected, Action<int> pick)
    {
        int[] kept = Enumerable.Range(0, items.Length).Where(i => !items[i].EndsWith("...") && !items[i].EndsWith('…')).ToArray();
        int Position() => Math.Max(0, Array.IndexOf(kept, selected()));
        return new()
        {
            Key = key, Type = "choice", Label = label, Choices = kept.Select(i => items[i]).ToArray(), Default = Position(),
            Apply = v => { int i = Whole(v); if (i >= 0 && i < kept.Length && kept[i] != selected()) pick(kept[i]); },
            Same = v => Whole(v) == Position(),
        };
    }

    /// Whether pressing `b` does something paint.rs can show: not closing the window (paint.rs's own OK and Cancel do
    /// that), showing help, or opening a file chooser, which the host cannot answer.
    private static bool Pressable(Button b)
    {
        // ponytail: help and closing buttons are told by their text; a plug-in that names them otherwise shows them.
        if (Plain(b.Text).ToLowerInvariant() is "ok" or "cancel" or "close" or "help" or "about" or "?") return false;
        static bool Unanswerable(MethodBase m) => m.DeclaringType is { } t
            && (typeof(FileDialog).IsAssignableFrom(t) || t == typeof(FolderBrowserDialog)
                || (typeof(Form).IsAssignableFrom(t) && m.Name is "Close" or "set_DialogResult"));
        foreach (string name in ClickEvents)
            if (Handlers(b, name) is { } h && h.List.Any(d => Calls(d.Method, Unanswerable, 0)))
                return false;
        return true;
    }

    /// A list item as its list shows it; an item that shows only its type's name (a plug-in that draws its items
    /// itself) as its simple properties ("Width 4, Spacing 16, Style 0, Color #000000").
    private static string Describe(ListBox list, object? item)
    {
        string text = list.GetItemText(item);
        if (item is null || text != item.GetType().FullName) return text;
        // ponytail: a "...Html" property is skipped as the text form of another one (a colour); real names may vary.
        return string.Join(", ", item.GetType().GetProperties(BindingFlags.Instance | BindingFlags.Public)
            .Where(p => p.GetIndexParameters().Length == 0 && !p.Name.EndsWith("Html", StringComparison.Ordinal)
                        && (p.PropertyType.IsPrimitive || p.PropertyType.IsEnum || p.PropertyType == typeof(string) || p.PropertyType == typeof(Color)))
            .Select(p => $"{Humanize(p.Name)} {(p.GetValue(item) is Color c ? $"#{c.R:X2}{c.G:X2}{c.B:X2}" : p.GetValue(item))}"));
    }

    private static string[] Texts(System.Collections.IList items, Func<object?, string> text)
        => Enumerable.Range(0, items.Count).Select(i => text(items[i])).ToArray();

    /// A swatch the user clicks to pick a colour: the colour it opens its dialog with is the default, and a
    /// replayed colour answers that dialog.
    private static Setting ColourPicker(string key, string label, Control c, List<(EventInfo, Delegate)> handlers, bool probe)
    {
        ColorDialog.HostAnswer = null;
        ColorDialog.HostLastOffered = null;
        if (probe) Raise(c, handlers);   // unanswered: the dialog is cancelled and nothing changes
        Color offered = ColorDialog.HostLastOffered ?? c.BackColor;
        return new Setting
        {
            Key = key, Type = "color", Label = label, Color = offered,
            Apply = v =>
            {
                ColorDialog.HostAnswer = ParseColour(v);
                try { Raise(c, handlers); } finally { ColorDialog.HostAnswer = null; }
            },
        };
    }

    /// A colour setting's value: "#rrggbb", or the app's primary or secondary colour.
    internal static Color ParseColour(JsonElement v) => v.GetString() switch
    {
        "primary" => HostState.Primary.ToColor(),
        "secondary" => HostState.Secondary.ToColor(),
        string hex when hex.StartsWith('#') && int.TryParse(hex[1..], NumberStyles.HexNumber, null, out int rgb) => Color.FromArgb(255, Color.FromArgb(rgb)),
        _ => throw new FormatException("not a colour"),
    };

    private static readonly string[] ClickEvents = ["Click", "MouseClick", "MouseDown", "MouseUp", "DoubleClick"];

    /// The handlers on `c`'s click events that open a colour dialog (null when none do).
    private static List<(EventInfo, Delegate)>? PicksColour(Control c)
    {
        var found = new List<(EventInfo, Delegate)>();
        foreach (string name in ClickEvents)
            if (Handlers(c, name) is { } h && h.Event is { } ev)
                foreach (Delegate d in h.List.Where(d => Calls(d.Method, callee => callee.DeclaringType == typeof(ColorDialog), 0)))
                    found.Add((ev, d));
        return found.Count > 0 ? found : null;
    }

    /// The handlers on `c`'s click events that open a file-open dialog (null when none do).
    private static List<(EventInfo, Delegate)>? OpensFile(Control c)
    {
        var found = new List<(EventInfo, Delegate)>();
        foreach (string name in ClickEvents)
            if (Handlers(c, name) is { } h && h.Event is { } ev)
                foreach (Delegate d in h.List.Where(d => Calls(d.Method, callee => callee.DeclaringType == typeof(OpenFileDialog) || callee.DeclaringType == typeof(FileDialog), 0)))
                    found.Add((ev, d));
        return found.Count > 0 ? found : null;
    }

    /// A button that opens a file chooser: the user's chosen file (its name and bytes, which paint.rs sends as
    /// { "name", "bytes" (base64) }) is replayed into the plug-in's OpenFileDialog, as ColourPicker replays a
    /// colour. The probe runs the handler unanswered, so its dialog cancels and we learn its filter and title.
    private static Setting FilePicker(string key, string label, Control c, List<(EventInfo, Delegate)> handlers, bool probe)
    {
        FileDialog.ClearHostAnswer();
        FileDialog.HostLastOffered = null;
        if (probe) try { Raise(c, handlers); } catch (Exception) { }   // its dialog cancels; just learns the filter
        FileDialog? offered = FileDialog.HostLastOffered;
        string title = Plain(c.Text) is { Length: > 1 } t ? t
                     : offered is not null && offered.Title.Length > 0 ? offered.Title
                     : label;
        return new Setting
        {
            Key = key, Type = "file", Label = title, Text = offered?.Filter ?? "", Named = true,
            Apply = v =>
            {
                if (v.ValueKind != JsonValueKind.Object) return;
                byte[] bytes = v.TryGetProperty("bytes", out JsonElement b) && b.GetString() is { } s64 ? Convert.FromBase64String(s64) : [];
                string name = v.TryGetProperty("name", out JsonElement n) ? n.GetString() ?? "" : "";
                FileDialog.SetHostAnswer(bytes, name);
                try { Raise(c, handlers); } finally { FileDialog.ClearHostAnswer(); }
            },
        };
    }

    private static (EventInfo? Event, Delegate[] List)? Handlers(Control c, string eventName)
    {
        for (Type? t = c.GetType(); t is not null; t = t.BaseType)
            if (t.GetField(eventName, BindingFlags.Instance | BindingFlags.NonPublic | BindingFlags.DeclaredOnly)?.GetValue(c) is Delegate d)
                return (t.GetEvent(eventName), d.GetInvocationList());
        return null;
    }

    /// Runs a user's click on `c` through the handlers that pick its colour.
    private static void Raise(Control c, List<(EventInfo Event, Delegate Handler)> handlers)
    {
        foreach ((EventInfo ev, Delegate d) in handlers)
            d.DynamicInvoke(c, ev.EventHandlerType == typeof(MouseEventHandler)
                ? new MouseEventArgs(MouseButtons.Left, 1, c.Width / 2, c.Height / 2, 0) : EventArgs.Empty);
    }

    /// Whether `m`, or a method of its plug-in it calls (a few calls deep), calls a method `wanted` picks (a
    /// ColorDialog's, to read or set its colour). The call tokens are found by scanning the IL for call, callvirt and
    /// newobj; a stray byte that happens to look like one only resolves to an unrelated method.
    private static bool Calls(MethodBase m, Func<MethodBase, bool> wanted, int depth)
    {
        byte[]? il;
        try { il = m.GetMethodBody()?.GetILAsByteArray(); }
        catch (Exception) { return false; }   // a body naming a type the host lacks: it cannot run here either
        if (depth > 3 || il is null) return false;
        for (int i = 0; i + 4 < il.Length; i++)
        {
            if (il[i] is not (0x28 or 0x6F or 0x73)) continue;
            int token = BitConverter.ToInt32(il, i + 1);
            if ((token >> 24) is not (0x06 or 0x0A or 0x2B)) continue;
            MethodBase? callee;
            try { callee = m.Module.ResolveMethod(token, m.DeclaringType?.GetGenericArguments(), m.IsGenericMethod ? m.GetGenericArguments() : null); }
            catch (Exception) { continue; }
            if (callee is not null && wanted(callee)) return true;
            if (callee is not null && callee.Module == m.Module && callee != m && Calls(callee, wanted, depth + 1)) return true;
        }
        return false;
    }

    private static bool IsPluginControl(Control c)
    {
        string? asm = c.GetType().Assembly.GetName().Name;
        return asm is not null && asm != "System.Windows.Forms" && !asm.StartsWith("PaintDotNet", StringComparison.Ordinal);
    }

    private static readonly string[] ColourNames = ["Value", "Color", "SelectedColor", "CurrentColor", "PrimaryColor", "UserColor"];

    /// A plug-in's own control with a value of its own: a number between a minimum and a maximum, or a colour.
    private static Setting? OwnValue(Control c, string key, string label)
    {
        Type t = c.GetType();
        PropertyInfo? Prop(string name) => t.GetProperty(name, BindingFlags.Instance | BindingFlags.Public);
        if (Prop("Value") is { CanWrite: true, CanRead: true } value && IsNumber(value.PropertyType)
            && (Prop("Minimum") ?? Prop("MinValue")) is { CanRead: true } min && (Prop("Maximum") ?? Prop("MaxValue")) is { CanRead: true } max)
        {
            double Get(PropertyInfo p) => Convert.ToDouble(p.GetValue(c), CultureInfo.InvariantCulture);
            bool whole = value.PropertyType == typeof(int) || value.PropertyType == typeof(long);
            // Its own decimals and step when it has them, as a numeric box does (a getter that throws counts as none).
            double? Optional(string name) { try { return Prop(name) is { CanRead: true } q && IsNumber(q.PropertyType) ? Get(q) : null; } catch (Exception) { return null; } }
            int decimals = whole ? 0 : (int)(Optional("DecimalPlaces") ?? 2);
            double step = Optional("Increment") ?? Optional("SmallChange") ?? 0.01;
            return Range(key + "value", label, Get(min), Get(max), Get(value), decimals, step,
                         v => value.SetValue(c, Convert.ChangeType(whole ? Math.Round(v) : v, value.PropertyType, CultureInfo.InvariantCulture)),
                         () => Get(value), false);
        }
        foreach (string name in ColourNames)
            if (Prop(name) is { CanWrite: true, CanRead: true } p && p.PropertyType == typeof(Color))
                return new Setting
                {
                    Key = key + "color", Type = "color", Label = label, Color = (Color)p.GetValue(c)!,
                    Apply = v => p.SetValue(c, ParseColour(v)),
                    Same = v => ParseColour(v).ToArgb() == ((Color)p.GetValue(c)!).ToArgb(),
                };
        return null;
    }

    private static bool IsNumber(Type t) => t == typeof(int) || t == typeof(long) || t == typeof(float) || t == typeof(double) || t == typeof(decimal);

    /// What the window calls a control. A caption is a label at most about two lines tall: the one to its left beside
    /// its first line, the header over most of its width, or both for a table's cell ("Set all" + "Distance"); within
    /// 80 px, except along a table's rows and columns. Else a tall label boxing it in, or its group box, names its
    /// group. Null when it shows none.
    private static string? Caption(Control c)
    {
        if (c.Parent is not { } parent) return null;
        Rectangle b = c.Bounds;
        int lines = Math.Max(Math.Min(b.Height, 24), 20) * 2, mid = b.Top + b.Height / 2;
        // A row label sits beside the control's first line (a track bar's thumb runs along its top).
        int firstLine = b.Top + Math.Min(b.Height, 24);
        Control? row = null, column = null;
        string? box = null;
        foreach (Control s in parent.Controls)
        {
            if (s == c || !IsCaption(s) || !s.Visible || Plain(s.Text).Length == 0) continue;
            Rectangle lb = s.Bounds;
            if (lb.Height <= lines && lb.Top < firstLine && lb.Bottom > b.Top && lb.Right <= b.Left + 4)
            {
                if (row is null || lb.Right > row.Bounds.Right) row = s;
            }
            else if (lb.Height <= lines && lb.Bottom <= b.Top + 4 && 2 * Overlap(lb, b) > Math.Min(lb.Width, b.Width))
            {
                if (column is null || lb.Bottom > column.Bounds.Bottom) column = s;
            }
            else if (lb.Height > lines && lb.Contains(b.Left + b.Width / 2, mid))
                box ??= Name(s);
        }
        // A header heads a table's column when three or more controls of this one's kind run down under it, each within
        // two lines of the last, through this one. There a header may be any distance up and the row's label far left;
        // elsewhere both are within 80 px.
        bool table = false;
        if (column is not null)
        {
            int bottom = column.Bottom, cells = 0;
            foreach (Control o in parent.Controls.Cast<Control>().Where(o => o.GetType() == c.GetType() && o.Top >= column.Bottom - 4
                && 2 * Overlap(column.Bounds, o.Bounds) > Math.Min(column.Width, o.Width)).OrderBy(o => o.Top))
            {
                if (o.Top - bottom > lines) break;
                (bottom, cells, table) = (o.Bottom, cells + 1, table || o == c);
            }
            table &= cells >= 3;
            if (!table && b.Top - column.Bottom > 80) column = null;
        }
        if (row is not null && b.Left - row.Right > 80 && !table) row = null;
        if (row is not null && table) return $"{Name(row)} {Name(column!)}";
        if (row is not null) return Name(row);
        if (column is not null) return Name(column);
        if (box is not null) return box;
        return parent is GroupBox g && Plain(g.Text).Length > 0 ? Plain(g.Text) : null;

        static string Name(Control label) => Regex.Replace(Plain(label.Text), @"\s+", " ").TrimEnd(' ', ':');
        static int Overlap(Rectangle a, Rectangle o) => Math.Max(0, Math.Min(a.Right, o.Right) - Math.Max(a.Left, o.Left));
    }

    /// What a control shows as its name: its text, else its tooltip, else its name.
    private static string Shown(Control c)
        => Plain(c.Text) is { Length: > 0 } t ? t : Plain(ToolTip.HostTipOf(c)) is { Length: > 0 } tip ? tip : Humanize(c.Name);

    /// A label, Paint.NET's header label, or a plug-in's own label control.
    private static bool IsCaption(Control c) => c is Label || c.GetType().Name.EndsWith("Label", StringComparison.Ordinal);

    private static string Plain(string? text) => (text ?? "").Replace("&&", "\u0001").Replace("&", "").Replace('\u0001', '&').Trim();

    private static readonly Regex Prefix = new("^(trackBar|trackbar|numericUpDown|numeric|nud|num|checkBox|chk|cb|comboBox|cmb|cbo|textBox|txt|tb|radioButton|rb|rdo|button|btn|panel|pnl|pictureBox|pb|lbl|label)(?=[A-Z0-9])");

    private static readonly Regex Suffix = new("(?<=[a-z0-9])(TrackBar|NumericUpDown|NumericSpin|Numeric|UpDown|Spin|Slider|Nud|CheckBox|Check|ComboBox|Combo|TextBox|Button|Picker|Panel)$", RegexOptions.IgnoreCase);

    /// A control name as words: "trackBarRadius2" reads "Radius 2".
    private static string Humanize(string name)
    {
        string bare = Suffix.Replace(Prefix.Replace(name, ""), "");
        string words = Regex.Replace(bare, "(?<=[a-z])(?=[A-Z0-9])|(?<=[0-9])(?=[A-Za-z])|_", " ").Trim();
        return words.Length == 0 ? "Setting" : char.ToUpperInvariant(words[0]) + words[1..];
    }

    /// A user's click on a check box or radio button: it changes, then Click is raised.
    // The click as Windows Forms raises it: the plug-in's own exception (not the reflection call's wrapper), so its
    // message is the one the user reads.
    private static void Click(ButtonBase b)
    {
        try { OnClick.Invoke(b, [EventArgs.Empty]); }
        catch (TargetInvocationException ex) when (ex.InnerException is { } inner)
        {
            System.Runtime.ExceptionServices.ExceptionDispatchInfo.Capture(inner).Throw();
        }
    }
    private static readonly MethodInfo OnClick = typeof(Control).GetMethod("OnClick", BindingFlags.Instance | BindingFlags.NonPublic)!;
}
