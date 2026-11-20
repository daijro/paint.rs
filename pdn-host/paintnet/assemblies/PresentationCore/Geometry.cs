// The parts of WPF's PresentationCore geometry plug-ins use, for hosts without WPF. Our own code. Radius Corners
// hit-tests an EllipseGeometry; CodeLab 6.x's ShapeBuilder validates a shape path through Geometry.Parse.
using System;
using System.Globalization;
using PaintDotNet.Direct2D1;

namespace System.Windows.Media;

public abstract class Geometry
{
    private protected Geometry() { }
    /// Whether the filled geometry contains the point.
    public bool FillContains(Point hitPoint) => Contains(hitPoint);
    public bool FillContains(Point hitPoint, double tolerance, ToleranceType type) => Contains(hitPoint);
    private protected abstract bool Contains(Point p);

    /// Parses the WPF path mini-language ("abbreviated geometry"), throwing FormatException on bad syntax as WPF does.
    /// CodeLab's ShapeBuilder uses the result only as a validity token (a StreamGeometry; its data is not read back).
    public static Geometry Parse(string abbreviatedGeometry)
    {
        new PathMarkupParser(abbreviatedGeometry ?? throw new ArgumentNullException(nameof(abbreviatedGeometry))).Parse();
        return new StreamGeometry();
    }

    /// The geometry's transform. ponytail: the stand-in geometries carry none (CodeLab uses Parse's result only as a
    /// validity token), so this is null; give it a backing if a plug-in reads the matrix.
    public Transform? Transform => null;

    /// The host Direct2D geometry this renders through (DrawingContext.DrawGeometry fills/strokes it); null when the
    /// stand-in carries no renderable path (e.g. StreamGeometry, used only as a parse validity token).
    internal virtual IGeometry? BuildHost(IDirect2DFactory factory) => null;
}

/// WPF's transform base (System.Windows.Media). The stand-in needs the type as Geometry.Transform's return; a
/// geometry here never carries one, so Value is the identity matrix.
public abstract class Transform { public virtual Matrix Value => Matrix.Identity; }

public enum ToleranceType { Absolute, Relative }

public sealed class EllipseGeometry : Geometry
{
    public EllipseGeometry() { }
    public EllipseGeometry(Point center, double radiusX, double radiusY) => (Center, RadiusX, RadiusY) = (center, radiusX, radiusY);
    public Point Center { get; set; }
    public double RadiusX { get; set; }
    public double RadiusY { get; set; }
    private protected override bool Contains(Point p)
    {
        if (RadiusX == 0 || RadiusY == 0) return false;
        double dx = (p.X - Center.X) / RadiusX, dy = (p.Y - Center.Y) / RadiusY;
        return dx * dx + dy * dy <= 1;
    }
    internal override IGeometry? BuildHost(IDirect2DFactory factory)
    {
        if (RadiusX <= 0 || RadiusY <= 0) return null;
        const int N = 64;
        var pts = new System.Drawing.PointF[N];
        for (int i = 0; i < N; i++) { double a = 2 * Math.PI * i / N; pts[i] = new System.Drawing.PointF((float)(Center.X + RadiusX * Math.Cos(a)), (float)(Center.Y + RadiusY * Math.Sin(a))); }
        return WpfPathGeometry.Create(factory, new[] { pts });
    }
}

/// What Geometry.Parse returns. A validity token for a parsed path; its figures are not exposed (CodeLab reads none).
public sealed class StreamGeometry : Geometry
{
    private protected override bool Contains(Point p) => false;
}

/// Validates the WPF path mini-language grammar. Commands and their coordinate counts:
/// F<fillrule>  M/m x,y  L/l x,y  H/h x  V/v y  C/c x1,y1,x2,y2,x,y  Q/q x1,y1,x,y  S/s x2,y2,x,y
/// T/t x,y  A/a rx,ry,rot,large,sweep,x,y  Z/z. Numbers repeat for a command (an extra set is another segment).
internal sealed class PathMarkupParser
{
    private readonly string s;
    private int i;
    public PathMarkupParser(string text) { s = text; }

    public void Parse()
    {
        SkipSeparators();
        if (i < s.Length && (s[i] == 'F' || s[i] == 'f')) { i++; ReadNumber(); }   // optional fill rule
        char command = '\0';
        bool any = false;
        while (true)
        {
            SkipSeparators();
            if (i >= s.Length) break;
            char c = s[i];
            if (IsCommand(c)) { command = c; i++; any = true; }
            else if (command == '\0') throw Bad($"unexpected '{c}'");   // a number before any command
            ReadArgs(command);
        }
        if (!any) throw Bad("no path commands");
    }

    private void ReadArgs(char command)
    {
        int n = ArgCount(command);
        for (int k = 0; k < n; k++)
        {
            if (command is 'A' or 'a' && (k == 3 || k == 4)) ReadFlag();   // large-arc and sweep are 0/1 flags
            else ReadNumber();
        }
    }

    private static bool IsCommand(char c) => "MmLlHhVvCcQqSsTtAaZz".IndexOf(c) >= 0;

    private static int ArgCount(char c) => char.ToUpperInvariant(c) switch
    {
        'M' or 'L' or 'T' => 2, 'H' or 'V' => 1, 'C' => 6, 'Q' or 'S' => 4, 'A' => 7, 'Z' => 0,
        _ => throw new FormatException($"unknown path command '{c}'"),
    };

    private void ReadFlag()
    {
        SkipSeparators();
        if (i >= s.Length || (s[i] != '0' && s[i] != '1')) throw Bad("expected a 0 or 1 flag");
        i++;
    }

    private void ReadNumber()
    {
        SkipSeparators();
        int start = i;
        if (i < s.Length && (s[i] == '+' || s[i] == '-')) i++;
        int digits = 0;
        while (i < s.Length && char.IsDigit(s[i])) { i++; digits++; }
        if (i < s.Length && s[i] == '.') { i++; while (i < s.Length && char.IsDigit(s[i])) { i++; digits++; } }
        if (i < s.Length && (s[i] == 'e' || s[i] == 'E'))
        {
            i++;
            if (i < s.Length && (s[i] == '+' || s[i] == '-')) i++;
            if (i >= s.Length || !char.IsDigit(s[i])) throw Bad("malformed exponent");
            while (i < s.Length && char.IsDigit(s[i])) i++;
        }
        if (digits == 0) throw Bad("expected a number");
        if (!double.TryParse(s.AsSpan(start, i - start), NumberStyles.Float, CultureInfo.InvariantCulture, out _))
            throw Bad($"'{s.Substring(start, i - start)}' is not a number");
    }

    private void SkipSeparators() { while (i < s.Length && (char.IsWhiteSpace(s[i]) || s[i] == ',')) i++; }

    private FormatException Bad(string why) => new FormatException($"invalid path markup at {i}: {why}");
}
