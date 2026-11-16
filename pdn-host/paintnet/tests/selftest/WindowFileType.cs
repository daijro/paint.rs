// A classic FileType that asks for its options in windows of its own while it saves and opens, as TechnoRobbo's
// Icons and Midora's EMF do: saving shows a window (a size and a check box, the box read in its OK button's handler)
// and writes what it was set to; opening shows one over Paint.NET's main window, found as Midora finds it, and
// makes an image the width it was set to. Our own code.
extern alias pdndata;
using System;
using System.Drawing;
using System.IO;
using System.Windows.Forms;
using PaintDotNet;
using Document = pdndata::PaintDotNet.Document;
using BitmapLayer = pdndata::PaintDotNet.BitmapLayer;
using FileType = pdndata::PaintDotNet.FileType;
using FileTypeFlags = pdndata::PaintDotNet.FileTypeFlags;
using SaveConfigToken = pdndata::PaintDotNet.SaveConfigToken;

namespace PdnSelfTest;

public sealed class WindowFileTypeFactory : pdndata::PaintDotNet.IFileTypeFactory
{
    public FileType[] GetFileTypeInstances() => [new WindowFileType()];
}

/// Saves two bytes, the window's size and whether "Merged" was ticked; opens to a (width)×1 image, red when "Red"
/// was ticked, else blue.
public sealed class WindowFileType() : FileType("Selftest Windows", FileTypeFlags.SupportsLoading | FileTypeFlags.SupportsSaving, [".pdnwin"])
{
    protected override void OnSave(Document input, Stream output, SaveConfigToken token, Surface scratchSurface, ProgressEventHandler progressCallback)
    {
        using var window = new SaveWindow();
        if (window.ShowDialog() != DialogResult.OK) return;   // saves nothing, as TechnoRobbo's do
        output.WriteByte((byte)window.SizeBox.Value);
        output.WriteByte((byte)(window.Merged ? 1 : 0));
    }

    protected override Document OnLoad(Stream input)
    {
        Form main = Application.OpenForms["MainForm"] ?? throw new InvalidOperationException("no main window");
        using var window = new OpenWindow();
        DialogResult answer = DialogResult.None;
        main.Invoke(new MethodInvoker(() => answer = window.ShowDialog(main)));
        if (answer != DialogResult.OK) throw new OperationCanceledException("Canceled by user");
        int w = (int)window.WidthBox.Value;
        var doc = new Document(w, 1);
        doc.Layers.Add(new BitmapLayer(w, 1, window.Red.Checked ? ColorBgra.Red : ColorBgra.Blue));
        return doc;
    }
}

internal sealed class SaveWindow : Form
{
    public readonly NumericUpDown SizeBox = new() { Minimum = 1, Maximum = 64, Value = 16, Location = new Point(60, 8) };
    private readonly Label sizeLabel = new() { Text = "Size:", Location = new Point(8, 10), Size = new Size(48, 16) };
    private readonly CheckBox merged = new() { Text = "Merged", Location = new Point(8, 36) };
    private readonly Button ok = new() { Text = "OK", DialogResult = DialogResult.OK, Location = new Point(100, 70) };
    private readonly Button cancel = new() { Text = "Cancel", DialogResult = DialogResult.Cancel, Location = new Point(180, 70) };
    public bool Merged;

    public SaveWindow()
    {
        Text = "Save Options";
        foreach (Control c in new Control[] { sizeLabel, SizeBox, merged, ok, cancel }) Controls.Add(c);
        (AcceptButton, CancelButton) = (ok, cancel);
        ok.Click += (_, _) => Merged = merged.Checked;
    }
}

internal sealed class OpenWindow : Form
{
    public readonly NumericUpDown WidthBox = new() { Minimum = 1, Maximum = 100, Value = 3, Location = new Point(60, 8) };
    private readonly Label widthLabel = new() { Text = "Width:", Location = new Point(8, 10), Size = new Size(48, 16) };
    public readonly CheckBox Red = new() { Text = "Red", Location = new Point(8, 36) };
    private readonly Button ok = new() { Text = "OK", DialogResult = DialogResult.OK, Location = new Point(100, 70) };

    public OpenWindow()
    {
        Text = "Open Options";
        foreach (Control c in new Control[] { widthLabel, WidthBox, Red, ok }) Controls.Add(c);
        AcceptButton = ok;
    }
}
