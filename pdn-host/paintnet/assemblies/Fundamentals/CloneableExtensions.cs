// A typed Clone helper classic plug-ins call. It lives in PaintDotNet.Fundamentals (PSFilterPdn binds it there);
// PaintDotNet.Base forwards it so ScriptLab, which binds it in Base, resolves too.
using System;

namespace PaintDotNet;

public static class CloneableExtensions
{
    /// x.Clone() typed back to T (ICloneable.Clone returns object); a non-cloneable value is returned as is.
    public static T CloneT<T>(this T source) => source is ICloneable c ? (T)c.Clone() : source;
}
