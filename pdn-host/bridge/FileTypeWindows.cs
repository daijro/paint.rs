// A FileType's own windows while it opens or saves a file. Our own code.
using System;
using System.Collections.Generic;
using System.Linq;
using System.Text;
using System.Text.Json;
using System.Threading;
using System.Windows.Forms;

namespace PaintDotNet.Bridge;

/// The windows a FileType shows while it opens or saves (TechnoRobbo's Icons ask which sizes to write, Midora's EMF
/// what size to open at). paint.rs shows each as it shows an effect's window and opens or saves again with the
/// answers: the window shown k-th takes answer k, set as the user set it in paint.rs, and is OK'd. A window shown
/// before paint.rs has an answer for it is read and cancelled, and the open or save then stops with
/// WindowNeededException, which carries what paint.rs shows. (.NET in the browser cannot wait for the user in the
/// middle of a call, so the desktop and the browser both go round twice.)
internal sealed class FileTypeWindows
{
    private static readonly AsyncLocal<FileTypeWindows?> Current = new();
    private readonly JsonElement[] answers;
    private int shown;
    private bool reading;
    private string? needed;

    static FileTypeWindows() => Form.HostDialog = form => Current.Value?.Show(form);

    private FileTypeWindows(JsonElement[] answers) => this.answers = answers;

    /// Runs an open or save, `body`, with the windows' answers `propsJson` carries under "$windows" (a list, in the
    /// order the windows show).
    public static T Run<T>(string? propsJson, Func<T> body)
    {
        JsonElement[] answers = [];
        if (!string.IsNullOrEmpty(propsJson))
        {
            using JsonDocument doc = JsonDocument.Parse(propsJson);
            if (doc.RootElement.ValueKind == JsonValueKind.Object && doc.RootElement.TryGetProperty("$windows", out JsonElement w)
                && w.ValueKind == JsonValueKind.Array)
                answers = w.EnumerateArray().Select(a => a.Clone()).ToArray();
        }
        var windows = new FileTypeWindows(answers);
        FileTypeWindows? outer = Current.Value;
        Current.Value = windows;
        T result;
        // A FileType stops in its own way when its window is cancelled (an exception, or nothing saved).
        try { result = body(); }
        catch (Exception) when (windows.needed is not null) { result = default!; }
        finally { Current.Value = outer; }
        if (windows.needed is { } window) throw new WindowNeededException(window);
        return result;
    }

    private DialogResult? Show(Form form)
    {
        if (reading) return null;   // a window the one being read opens (a colour picker) is cancelled
        int k = shown++;
        form.RaiseLoad();
        if (k < answers.Length)
        {
            DialogSettings.Apply(form, answers[k]);
            return Accept(form);
        }
        if (needed is null)
        {
            reading = true;
            try { needed = Describe(form); }
            finally { reading = false; }
        }
        return null;
    }

    /// What paint.rs shows for the window: its title and its settings (docs/pdn-bridge.md "FileType windows"). Its
    /// buttons are left out: paint.rs answers a window as a whole.
    private static string Describe(Form form)
    {
        using var buffer = new System.IO.MemoryStream();
        using (var jw = new Utf8JsonWriter(buffer))
        {
            jw.WriteStartObject();
            jw.WriteString("title", form.Text ?? "");
            DialogSettings.WriteProperties(jw, DialogSettings.OfShown(form).Where(s => s.Type != "button").ToList());
            jw.WriteEndObject();
        }
        return Encoding.UTF8.GetString(buffer.ToArray());
    }

    /// OK, as a click on the window's OK button, so its handlers run (some read the window's settings there).
    private static DialogResult Accept(Form form)
    {
        form.DialogResult = DialogResult.None;
        Button? ok = form.AcceptButton as Button ?? Buttons(form).FirstOrDefault(b => b.DialogResult == DialogResult.OK);
        if (ok is not null)
        {
            ok.PerformClick();
            if (form.DialogResult == DialogResult.None && ok.DialogResult != DialogResult.None) form.DialogResult = ok.DialogResult;
        }
        return form.DialogResult == DialogResult.None ? DialogResult.OK : form.DialogResult;
    }

    private static IEnumerable<Button> Buttons(Control c)
    {
        foreach (Control child in c.Controls)
        {
            if (child is Button b) yield return b;
            foreach (Button nested in Buttons(child)) yield return nested;
        }
    }
}

/// An open or save stopped at a window paint.rs has not answered yet. Its message is "pdn-window:" and the window's
/// JSON, which the host passes to paint.rs whole.
internal sealed class WindowNeededException(string window) : Exception(Prefix + window)
{
    public const string Prefix = "pdn-window:";
}
