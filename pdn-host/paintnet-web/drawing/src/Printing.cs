// System.Drawing.Printing for the browser host, which has no printer. A PrintDocument runs its PrintPage handlers
// against an in-memory page Graphics (our GDI+-less drawing over a bitmap), collects the rendered pages as PNG, and
// hands them to the page's own print, which the host wires as the "System.Drawing.Printing.PrintPages" app-context
// entry (as it wires the codecs in Codecs.cs). Without that entry the pages render but nothing is shown, so a
// printing plug-in still loads, lists and runs rather than throwing.
using System.Collections.Generic;
using System.Drawing.Imaging;
using System.IO;

namespace System.Drawing.Printing
{
    public partial class PrintDocument
    {
        public PrintDocument() { }

        public PrinterSettings PrinterSettings { get; set; } = new PrinterSettings();

        private PrintPageEventHandler _printPage;
        public event PrintPageEventHandler PrintPage { add => _printPage += value; remove => _printPage -= value; }

        // ponytail: a fixed Letter page in GDI+'s 1/100-inch page units (850x1100) with 1-inch margins, one unit per
        // pixel (no printer DPI or page-unit transform). Enough for a plug-in that lays its content out within
        // MarginBounds; raise to the printer's resolution and scale the page Graphics when print sharpness matters.
        public void Print()
        {
            PrintPageEventHandler handler = _printPage;
            if (handler == null) return;
            Rectangle pageBounds = new Rectangle(0, 0, 850, 1100);
            Rectangle marginBounds = new Rectangle(100, 100, 650, 900);
            List<byte[]> pages = new List<byte[]>();
            bool more = true;
            // ponytail: 1000-page guard against a handler that never clears HasMorePages.
            for (int page = 0; more && page < 1000; page++)
            {
                Bitmap bmp = new Bitmap(pageBounds.Width, pageBounds.Height);
                bool cancelled;
                using (Graphics g = Graphics.FromImage(bmp))
                {
                    g.Clear(Color.FromArgb(255, 255, 255));
                    PrintPageEventArgs e = new PrintPageEventArgs(g, marginBounds, pageBounds, null);
                    handler(this, e);
                    more = e.HasMorePages;
                    cancelled = e.Cancel;
                }
                if (!cancelled)
                    using (MemoryStream ms = new MemoryStream()) { bmp.Save(ms, ImageFormat.Png); pages.Add(ms.ToArray()); }
                bmp.Dispose();
                if (cancelled) break;
            }
            if (pages.Count > 0 && AppContext.GetData("System.Drawing.Printing.PrintPages") is Action<byte[][]> print)
                print(pages.ToArray());
        }
    }

    public partial class PrinterSettings
    {
        public PrinterSettings() { }
    }

    public partial class PrintPageEventArgs
    {
        public PrintPageEventArgs(Graphics graphics, Rectangle marginBounds, Rectangle pageBounds, PageSettings pageSettings)
        {
            Graphics = graphics;
            MarginBounds = marginBounds;
            PageBounds = pageBounds;
        }
        public Graphics Graphics { get; }
        public Rectangle MarginBounds { get; }
        public Rectangle PageBounds { get; }
        public bool HasMorePages { get; set; }
        public bool Cancel { get; set; }
    }
}
