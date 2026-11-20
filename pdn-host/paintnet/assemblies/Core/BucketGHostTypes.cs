// Host-service + helper types pre-5.2 plug-ins bind in PaintDotNet.Core. Real where a plug-in can observe the value
// (EXIF conversions, the angle value); inert (ponytail-noted) where the host owns the service or it is pure UI.
using System;
using System.Collections.Generic;
using System.Windows.Forms;

namespace PaintDotNet
{
    // A dial for picking an angle (Spiro plug-ins). ponytail: inert UI — stores the value and raises the event; real
    // = the drawn dial via the dialog bridge.
    public class AngleChooserControl : Control
    {
        private double angle;
        public AngleChooserControl() { }
        public int Value { get => (int)Math.Round(angle); set { angle = value; ValueChanged?.Invoke(this, EventArgs.Empty); } }
        public double ValueDouble { get => angle; set { angle = value; ValueChanged?.Invoke(this, EventArgs.Empty); } }
        public event EventHandler? ValueChanged;
    }

    // A using-scoped busy-cursor swap (Curves+/Smudge). ponytail: inert — no cursor on the headless/web path.
    public sealed class WaitCursorChanger : IDisposable
    {
        public WaitCursorChanger(Control control) { }
        public void Dispose() { }
    }
}

namespace PaintDotNet.Imaging
{
    // An IPTC string value read off a metadata item. ponytail: host keeps no IPTC store on this path, so it is empty.
    public sealed class IptcStringValue
    {
        public string Value { get; } = "";
    }

    // EXIF tag encode/decode PSFilterPdn uses; little-endian like the EXIF spec. Real.
    public static class ExifConverter
    {
        public static ExifValue EncodeShort(ushort value) => new ExifValue(ExifValueType.Short, new byte[] { (byte)value, (byte)(value >> 8) });
        public static ushort DecodeShort(IReadOnlyList<byte> data) => data.Count >= 2 ? (ushort)(data[0] | (data[1] << 8)) : (ushort)0;
        public static char[] DecodeAscii(IReadOnlyList<byte> data)
        {
            var chars = new char[data.Count];
            for (int i = 0; i < data.Count; i++) chars[i] = (char)data[i];
            return chars;
        }
    }
}

namespace PaintDotNet.AppModel
{
    public enum PluginType { Unknown = 0, Effect = 1, FileType = 2 }
    public enum CheckerboardTileSize { Default = 0, Small = 1, Medium = 2, Large = 3 }

    public sealed class PluginDirectoryInfo
    {
        // ponytail: host resolves plug-in paths itself; empty on this path.
        public string Path { get; } = "";
    }

    public interface IFileSystemService { }

    public interface ICheckerboardVisualStyling
    {
        PaintDotNet.Rendering.SizeInt32 GetRepeatingBitmapSize(CheckerboardTileSize tileSize);
    }

    public interface IVisualStylingService
    {
        ICheckerboardVisualStyling Checkerboard { get; }
    }

    // ponytail: host owns plug-in discovery + the checkerboard; inert (no dirs, no-op render) — the plug-in still loads.
    public static class FileSystemServiceExtensions
    {
        public static IEnumerable<PluginDirectoryInfo> GetPluginDirectoryInfos(this IFileSystemService service, PluginType pluginType)
            => Array.Empty<PluginDirectoryInfo>();
    }

    public static class CheckerboardVisualStylingExtensions
    {
        public static void Render(this ICheckerboardVisualStyling styling, PaintDotNet.RegionPtr<PaintDotNet.Imaging.ColorBgr32> region, CheckerboardTileSize tileSize) { }
    }
}
