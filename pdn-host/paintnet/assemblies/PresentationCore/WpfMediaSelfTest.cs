// Host-side WPF-Media render smoke + leak test. PdnBridge `wpftest` invokes Run through a PluginLoadContext (so these
// System.Windows.* types resolve to the stand-ins exactly as they do for a plug-in) over the pinned opend2d
// libd2d1/libdwrite. It renders a transform + filled/stroked rectangle + a path geometry + an optional glyph run N
// times, asserting the fill pixel and bounding native-resource growth — the guard for the per-op factory leak /
// null-buffer crash that compile-only checks miss.
using System;
using System.Diagnostics;

namespace System.Windows.Media
{
    internal static class WpfMediaSelfTest
    {
        public static int Run(int iterations, string? fontPath)
        {
            const int W = 64, H = 48;
            GlyphRun? glyphRun = null;
            if (!string.IsNullOrEmpty(fontPath) && System.IO.File.Exists(fontPath))
            {
                var tf = new GlyphTypeface(new Uri(System.IO.Path.GetFullPath(fontPath)));
                ushort gi = tf.CharacterToGlyphMap.TryGetValue('A', out ushort g) ? g : (ushort)1;
                glyphRun = new GlyphRun(tf, 0, false, 24.0, new ushort[] { gi }, new System.Windows.Point(4, 40),
                    new double[] { 20.0 }, null!, null!, null!, null!, null!, null!);
            }
            // A path geometry: a closed figure with a line and a cubic bezier (exercises the flattener + DrawGeometry).
            var pg = new PathGeometry();
            var fig = new PathFigure { StartPoint = new System.Windows.Point(50, 6), IsClosed = true };
            fig.Segments.Add(new LineSegment { Point = new System.Windows.Point(60, 8) });
            fig.Segments.Add(new BezierSegment { Point1 = new System.Windows.Point(62, 20), Point2 = new System.Windows.Point(52, 34), Point3 = new System.Windows.Point(50, 40) });
            pg.Figures.Add(fig);

            var fill = new SolidColorBrush(Color.FromArgb(255, 200, 40, 40));
            var pen = new Pen(new SolidColorBrush(Color.FromArgb(255, 0, 0, 255)), 2.0);
            var pathBrush = new SolidColorBrush(Color.FromArgb(255, 0, 180, 0));
            var proc = Process.GetCurrentProcess();
            long baseRss = 0;
            int hits = 0;
            var sw = Stopwatch.StartNew();
            for (int i = 0; i < iterations; i++)
            {
                var visual = new DrawingVisual();
                var dc = visual.RenderOpen();
                dc.PushTransform(new TranslateTransform(2, 2));
                dc.DrawRectangle(fill, pen, new System.Windows.Rect(6, 6, 40, 30));
                dc.DrawGeometry(pathBrush, null, pg);
                if (glyphRun is not null) dc.DrawGlyphRun(new SolidColorBrush(Color.FromArgb(255, 0, 0, 0)), glyphRun);
                dc.Pop();
                dc.Close();
                var rtb = new Imaging.RenderTargetBitmap(W, H, 96, 96, PixelFormats.Pbgra32);
                rtb.Render(visual);
                var px = new byte[W * H * 4];
                rtb.CopyPixels(px, W * 4, 0);
                int o = (20 * W + 20) * 4;   // interior of the translated rect; straight BGRA => B,G,R,A
                if (px[o + 2] > 150 && px[o + 1] < 90 && px[o + 0] < 90) hits++;
                if (i == 50) { proc.Refresh(); baseRss = proc.WorkingSet64; }
            }
            sw.Stop();
            proc.Refresh();
            long growthMb = baseRss > 0 ? (proc.WorkingSet64 - baseRss) / (1024 * 1024) : 0;
            bool pixelsOk = hits == iterations;
            bool leakOk = baseRss == 0 || growthMb < 64;   // ponytail: 64 MB slack; a per-op native-factory leak over 200 iters blows past this

            // Save the rendered frame through the host's imaging codecs (BitmapEncoder.Save over the WIC drop-in) and
            // confirm valid PNG output -- the WpfMath save chain (render -> PngBitmapEncoder.Save -> PNG).
            var sv = new DrawingVisual();
            var svc = sv.RenderOpen();
            svc.DrawRectangle(fill, null, new System.Windows.Rect(0, 0, W, H));
            svc.Close();
            var svRtb = new Imaging.RenderTargetBitmap(W, H, 96, 96, PixelFormats.Pbgra32);
            svRtb.Render(sv);
            var enc = new Imaging.PngBitmapEncoder();
            enc.Frames.Add(Imaging.BitmapFrame.Create(svRtb));
            using var pngMs = new System.IO.MemoryStream();
            enc.Save(pngMs);
            byte[] png = pngMs.ToArray();
            bool saveOk = png.Length > 67 && png[0] == 0x89 && png[1] == 0x50 && png[2] == 0x4E && png[3] == 0x47;   // "\x89PNG"

            Console.WriteLine($"wpftest: {iterations} renders, {hits} fill-pixel hits, glyph={glyphRun is not null}, {sw.ElapsedMilliseconds} ms, RSS +{growthMb} MB (iter50->end)");
            Console.WriteLine($"wpftest: PngBitmapEncoder.Save -> {png.Length} bytes, png-signature={saveOk}");
            Console.WriteLine($"wpftest: {(pixelsOk && leakOk && saveOk ? "PASS" : "FAIL" + (pixelsOk ? "" : " [pixels]") + (leakOk ? "" : " [leak]") + (saveOk ? "" : " [save]"))}");
            return pixelsOk && leakOk && saveOk ? 0 : 1;
        }
    }
}
