// The slice of WPF's System.Windows.Markup that CodeLab 6.x's ShapeBuilder uses: XamlReader.Parse validates pasted
// geometry XAML (ShapeBuilder parses it to check it, reporting a XamlParseException's line and position on a malformed
// paste, and keeps the result as a validity token). This does a real XML well-formedness parse -- so a malformed paste
// is reported with the right line/position, not swallowed -- and returns a geometry for valid input. Only the members
// ShapeBuilder references are present.
using System;
using System.Xml;
using System.Xml.Linq;
using System.Windows.Media;

namespace System.Windows.Markup
{
    public static class XamlReader
    {
        /// Parses a XAML string. ShapeBuilder passes geometry XAML and casts the result to Geometry; a malformed paste
        /// raises XamlParseException with the line/position, as WPF does.
        public static object Parse(string xaml)
        {
            if (xaml is null) throw new ArgumentNullException(nameof(xaml));
            try { XDocument.Parse(xaml, LoadOptions.SetLineInfo); }
            catch (XmlException ex) { throw new XamlParseException(ex.Message, ex.LineNumber, ex.LinePosition); }
            return new StreamGeometry();
        }
    }

    public class XamlParseException : Exception
    {
        public XamlParseException() { }
        public XamlParseException(string message) : base(message) { }
        public XamlParseException(string message, Exception innerException) : base(message, innerException) { }
        public XamlParseException(string message, int lineNumber, int linePosition) : base(message)
        {
            LineNumber = lineNumber;
            LinePosition = linePosition;
        }
        public int LineNumber { get; }
        public int LinePosition { get; }
    }
}
