// Where Paint.NET plug-ins keep their settings and files, one case per way they find the place (paint.rs's survey of
// the common plug-ins), each written and read back: green when it works without a permission prompt, as in Paint.NET,
// which never asks. The last cases go the other way: paint.rs's own saved state (its permission grants), the user's
// real Documents, and framework methods writing outside every allowed folder must be refused, green when the files
// gate asks instead of letting them through. The writes aim at $PDN_SELFTEST_OUTSIDE (the test's own folder outside
// every exemption), so a gate that fails cannot leave files in a real home. Our own code.
#nullable enable
using System;
using System.Configuration;
using System.Drawing;
using System.IO;
using System.IO.Compression;
using System.Linq;
using System.Xml;
using System.Xml.Linq;
using PaintDotNet;
using PaintDotNet.AppModel;
using PaintDotNet.Effects;
using PaintDotNet.PropertySystem;

namespace PdnSelfTest;

public sealed class SettingsPathsEffect : PropertyBasedEffect
{
    public enum Case
    {
        AppInfoUserData, UserFiles, PdnInfoUserData, OldUserFiles, AppData, LocalAppData, Documents, UserAppDataPath,
        UserConfig, Temp, XDocumentInArea, BitmapInArea,
        GrantsUpFromUserData, GrantsAbsolute, RealDocuments, XmlDocumentOutside, BitmapOutside,
        StreamWriterOutside, FileInfoOutside, PathStringOps, ExistsProbes, XPathQuery, ExistingOutsideLooksAbsent,
        ListOutside, ZipInArea, ZipOutside, CopyInArea, CopyOutside,
    }

    private ColorBgra verdict;

    public SettingsPathsEffect() : base("Settings Paths", null, null, new EffectOptions()) { }

    protected override PropertyCollection OnCreatePropertyCollection()
        => new([new Int32Property("Case", 0, 0, Enum.GetValues<Case>().Length - 1)]);

    protected override void OnSetRenderInfo(PropertyBasedEffectConfigToken newToken, RenderArgs dstArgs, RenderArgs srcArgs)
    {
        bool passed;
        try { passed = Run((Case)newToken.GetProperty<Int32Property>("Case")!.Value); }
        catch (Exception) { passed = false; }
        verdict = passed ? ColorBgra.FromBgra(0, 255, 0, 255) : ColorBgra.FromBgra(0, 0, 255, 255);
        base.OnSetRenderInfo(newToken, dstArgs, srcArgs);
    }

    protected override void OnRender(Rectangle[] rois, int startIndex, int length)
    {
        for (int i = startIndex; i < startIndex + length; i++) DstArgs.Surface.Fill(rois[i], verdict);
    }

    private bool Run(Case c)
    {
        string Folder(Environment.SpecialFolder f) => Environment.GetFolderPath(f);
        switch (c)
        {
            case Case.AppInfoUserData: return RoundTrip(Path.Combine(AppInfo.UserDataDirectory, "PdnSelfTest"));
            case Case.UserFiles: return RoundTrip(Path.Combine(((IUserFilesService)Services.GetService(typeof(IUserFilesService))!).UserFilesPath!, "PdnSelfTest"));
            case Case.PdnInfoUserData: return RoundTrip(Path.Combine(PdnInfo.UserDataPath, "PdnSelfTest"));
            case Case.OldUserFiles: return RoundTrip(Path.Combine(Folder(Environment.SpecialFolder.MyDocuments), "paint.net User Files", "Palettes"));
            case Case.AppData: return RoundTrip(Path.Combine(Folder(Environment.SpecialFolder.ApplicationData), "PdnSelfTest"));
            case Case.LocalAppData: return RoundTrip(Path.Combine(Folder(Environment.SpecialFolder.LocalApplicationData), "PdnSelfTest"));
            case Case.Documents: return RoundTrip(Path.Combine(Folder(Environment.SpecialFolder.MyDocuments), "PdnSelfTest"));
            case Case.UserAppDataPath: return RoundTrip(System.Windows.Forms.Application.UserAppDataPath);
            case Case.UserConfig:
            {
                var settings = new SelfTestSettings { Value = "ok " + Environment.TickCount64 };
                string written = settings.Value;
                settings.Save();
                var again = new SelfTestSettings();
                again.Reload();
                // Kept in the plug-in area (the host's settings provider), not the framework's own per-user folder.
                string area = Environment.GetEnvironmentVariable("PDN_PLUGIN_FILES") ?? "";
                string kept = Path.Combine(area, "Settings");
                bool inArea = area.Length > 0 && Directory.Exists(kept)
                    && Directory.EnumerateFiles(kept, "*", SearchOption.AllDirectories).Any(f => File.ReadAllText(f).Contains(written));
                return again.Value == written && inArea;
            }
            case Case.Temp: return RoundTrip(Path.Combine(Path.GetTempPath(), "PdnSelfTest"));
            // Framework methods that take a path, into the area's Documents mirror: through the gate, and let through.
            case Case.XDocumentInArea:
            {
                string dir = Path.Combine(Folder(Environment.SpecialFolder.MyDocuments), "PdnSelfTest");
                Directory.CreateDirectory(dir);
                string file = Path.Combine(dir, "pdnselftest.xml");
                new XDocument(new XElement("ok")).Save(file);
                return XDocument.Load(file).Root?.Name == "ok";
            }
            case Case.BitmapInArea:
            {
                string dir = Path.Combine(Folder(Environment.SpecialFolder.MyDocuments), "PdnSelfTest");
                Directory.CreateDirectory(dir);
                string file = Path.Combine(dir, "pdnselftest.png");
                using (var b = new Bitmap(2, 2)) b.Save(file);
                using var back = new Bitmap(file);
                return back.Width == 2;
            }
            // paint.rs's saved state, two ways: one ".." out of the plug-in area (in paint.rs's layout, its data folder,
            // where app.ron is), and by its absolute path (from the environment, not GetFolderPath, which a plug-in's
            // rewrite answers with the area's mirrors).
            case Case.GrantsUpFromUserData: return Refused(Path.Combine(AppInfo.UserDataDirectory, "..", "app.ron"));
            case Case.GrantsAbsolute: return Refused(GrantsFile());
            case Case.RealDocuments: return Refused(RealDocuments("pdnselftest.txt"));
            // The same framework methods aimed at the user's real Documents: the gate must ask before any of them writes.
            case Case.XmlDocumentOutside:
                return RefusedWrite(Outside("pdnselftest.xml"), f => { var x = new XmlDocument(); x.LoadXml("<ok/>"); x.Save(f); });
            case Case.BitmapOutside:
                return RefusedWrite(Outside("pdnselftest.png"), f => { using var b = new Bitmap(2, 2); b.Save(f); });
            case Case.StreamWriterOutside:
                return RefusedWrite(Outside("pdnselftest.log"), f => { using var w = new StreamWriter(f, true); w.Write("x"); });
            case Case.FileInfoOutside:
                return RefusedWrite(Outside("pdnselftest.bin"), f => new FileInfo(f).Create().Dispose());
            // Framework methods that only work on the string, probe, or query XML, given a path-like argument outside
            // every allowed folder: nothing is read or written, so no prompt (plug-ins call these all the time).
            case Case.PathStringOps:
            {
                string f = Outside(Path.Combine("a", "pdnselftest.txt"));
                return Path.GetFileName(f) == "pdnselftest.txt" && Path.GetExtension(f) == ".txt"
                    && Path.GetFileNameWithoutExtension(f) == "pdnselftest" && Path.GetDirectoryName(f) is { Length: > 0 }
                    && Path.GetFullPath(f).Length > 0 && Path.ChangeExtension(f, ".png").EndsWith(".png", StringComparison.Ordinal);
            }
            case Case.ExistsProbes:
                return !File.Exists(Outside("pdnselftest-missing.txt")) && !Directory.Exists(Outside("pdnselftest-missing"));
            case Case.XPathQuery:
            {
                var x = new XmlDocument();
                x.LoadXml("<a><b/></a>");
                return x.SelectSingleNode("//b") is not null && x.SelectNodes("/a/b")?.Count == 1;
            }
            // A probe of a file that is there, outside every allowed folder (the test made it): it answers as if the
            // file were not there, and asks nothing.
            case Case.ExistingOutsideLooksAbsent:
                return !File.Exists(Outside("exists.txt")) && !new FileInfo(Outside("exists.txt")).Exists;
            // Listing a folder outside every allowed one asks.
            case Case.ListOutside: return RefusedWrite(Outside("listed"), _ => Directory.GetFiles(Outside("")));
            // ZipFile and File.Copy, both of whose path arguments are gated: within the area, through; out of it, asked.
            case Case.ZipInArea:
            {
                string area = AreaFolder("zip");
                string zip = Path.Combine(area, "..", "pdnselftest.zip");
                File.Delete(zip);
                ZipFile.CreateFromDirectory(area, zip);
                string unzipped = AreaFolder("unzipped");
                ZipFile.ExtractToDirectory(zip, unzipped, true);
                return File.Exists(Path.Combine(unzipped, "pdnselftest.txt"));
            }
            case Case.ZipOutside:
            {
                string area = AreaFolder("zip");
                string zip = Path.Combine(area, "..", "pdnselftest.zip");
                File.Delete(zip);
                ZipFile.CreateFromDirectory(area, zip);
                return RefusedWrite(Outside("unzipped"), d => ZipFile.ExtractToDirectory(zip, d, true));
            }
            case Case.CopyInArea:
            {
                string area = AreaFolder("copy");
                File.Copy(Path.Combine(area, "pdnselftest.txt"), Path.Combine(area, "copied.txt"), true);
                return File.ReadAllText(Path.Combine(area, "copied.txt")) == "ok";
            }
            case Case.CopyOutside:
            {
                string area = AreaFolder("copy");
                return RefusedWrite(Outside("copied.txt"), f => File.Copy(Path.Combine(area, "pdnselftest.txt"), f, true));
            }
            default: return false;
        }
    }

    /// A folder in the area's Documents mirror holding one file, pdnselftest.txt ("ok").
    private static string AreaFolder(string name)
    {
        string dir = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.MyDocuments), "PdnSelfTest", name);
        Directory.CreateDirectory(dir);
        File.WriteAllText(Path.Combine(dir, "pdnselftest.txt"), "ok");
        return dir;
    }

    private IAppInfoService AppInfo => (IAppInfoService)Services.GetService(typeof(IAppInfoService))!;

    /// Writes a file under `dir`, reads it back and removes it: true when all of it ran and nothing was asked.
    private static bool RoundTrip(string dir)
    {
        Directory.CreateDirectory(dir);
        string file = Path.Combine(dir, "pdnselftest.txt");
        File.WriteAllText(file, "ok");
        bool same = File.ReadAllText(file) == "ok";
        File.Delete(file);
        return same;
    }

    /// True when reading `file` is refused by the files gate (paint.rs would ask), not read or merely missing.
    private static bool Refused(string file)
    {
        try { File.ReadAllText(file); return false; }
        catch (Exception e) { return IsGate(e); }
    }

    private static string RealDocuments(string file) => Path.Combine(Environment.GetEnvironmentVariable("HOME") ?? "/", "Documents", file);

    /// A file outside every folder the gate lets through: the test's own ($PDN_SELFTEST_OUTSIDE), else the real Documents.
    private static string Outside(string file)
        => Environment.GetEnvironmentVariable("PDN_SELFTEST_OUTSIDE") is { Length: > 0 } dir ? Path.Combine(dir, file) : RealDocuments(file);

    private static bool IsGate(Exception e) => e.GetType().Name == "PermissionRequiredException" || e.Message.StartsWith("pdn-permission:");

    /// True when `write` to `file` is refused by the files gate before it writes anything.
    private static bool RefusedWrite(string file, Action<string> write)
    {
        try { write(file); return false; }
        catch (Exception e) { return (IsGate(e) || e.InnerException is { } i && IsGate(i)) && !File.Exists(file); }
    }

    /// eframe's saved-state file, where paint.rs keeps the permission grants.
    private static string GrantsFile()
    {
        string? Env(string k) => Environment.GetEnvironmentVariable(k) is { Length: > 0 } v ? v : null;
        if (OperatingSystem.IsWindows()) return Path.Combine(Env("APPDATA") ?? "", "paint.rs", "data", "app.ron");
        if (OperatingSystem.IsMacOS()) return Path.Combine(Env("HOME") ?? "", "Library", "Application Support", "paint.rs", "app.ron");
        return Path.Combine(Env("XDG_DATA_HOME") ?? Path.Combine(Env("HOME") ?? "", ".local", "share"), "paint.rs", "app.ron");
    }

    private sealed class SelfTestSettings : ApplicationSettingsBase
    {
        [UserScopedSetting, DefaultSettingValue("")]
        public string Value { get => (string)this[nameof(Value)]; set => this[nameof(Value)] = value; }
    }
}
