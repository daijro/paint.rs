// A plug-in's ApplicationSettingsBase (user.config) settings, kept inside the exempt plug-in area instead of the real
// LocalAppData. GateRewrite attaches [SettingsProvider(typeof(PluginSettingsProvider))] to the plug-in's settings
// classes; this stores each class's values as XML under <PluginArea>/Settings/<plug-in>/<group>.config. The framework
// writes user.config itself (not plug-in IL), so the files gate never saw it -- this just steers the location.
// ponytail: one XML file per settings class; the serialized value as text (base64 for a Binary property). Enough for
// the primitive/string settings plug-ins keep; revisit if a plug-in needs the framework's full user.config format.
using System;
using System.Collections.Generic;
using System.Collections.Specialized;
using System.Configuration;
using System.IO;
using System.Xml.Linq;

namespace PaintDotNet.Bridge;

public sealed class PluginSettingsProvider : SettingsProvider
{
    public override string ApplicationName { get; set; } = "";

    public override void Initialize(string name, NameValueCollection config)
        => base.Initialize(string.IsNullOrEmpty(name) ? nameof(PluginSettingsProvider) : name, config ?? new NameValueCollection());

    private static string FileFor(SettingsContext context)
    {
        string group = context["GroupName"] as string ?? context["SettingsClassType"]?.ToString() ?? "settings";
        foreach (char c in Path.GetInvalidFileNameChars()) group = group.Replace(c, '_');
        string dir = Path.Combine(GatedIo.PluginArea, "Settings", Gate.CurrentPlugin ?? "plugin");
        Directory.CreateDirectory(dir);
        return Path.Combine(dir, group + ".config");
    }

    public override SettingsPropertyValueCollection GetPropertyValues(SettingsContext context, SettingsPropertyCollection collection)
    {
        Dictionary<string, string> stored = Load(FileFor(context));
        var values = new SettingsPropertyValueCollection();
        foreach (SettingsProperty p in collection)
        {
            var v = new SettingsPropertyValue(p);
            if (stored.TryGetValue(p.Name, out string? s))
                v.SerializedValue = p.SerializeAs == SettingsSerializeAs.Binary ? Convert.FromBase64String(s) : s;
            else
                v.SerializedValue = p.DefaultValue;   // framework falls back to the [DefaultSettingValue]
            v.IsDirty = false;
            values.Add(v);
        }
        return values;
    }

    public override void SetPropertyValues(SettingsContext context, SettingsPropertyValueCollection collection)
    {
        var root = new XElement("settings");
        foreach (SettingsPropertyValue v in collection)
        {
            string text = v.SerializedValue is byte[] b ? Convert.ToBase64String(b) : v.SerializedValue?.ToString() ?? "";
            root.Add(new XElement("setting", new XAttribute("name", v.Name), text));
        }
        try { new XDocument(root).Save(FileFor(context)); } catch { }
    }

    private static Dictionary<string, string> Load(string file)
    {
        var d = new Dictionary<string, string>();
        try
        {
            if (File.Exists(file))
                foreach (XElement e in XDocument.Load(file).Root!.Elements("setting"))
                    if (e.Attribute("name")?.Value is { } name) d[name] = e.Value;
        }
        catch { }
        return d;
    }
}
