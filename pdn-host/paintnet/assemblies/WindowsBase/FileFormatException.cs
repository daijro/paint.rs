// WPF's error for a file whose contents are not in the format expected (null54's EA FSH FileType throws it).
namespace System.IO;

public class FileFormatException : FormatException
{
    public FileFormatException() { }
    public FileFormatException(string? message) : base(message) { }
    public FileFormatException(string? message, Exception? innerException) : base(message, innerException) { }
    public FileFormatException(Uri? sourceUri) => SourceUri = sourceUri;
    public FileFormatException(Uri? sourceUri, string? message) : base(message) => SourceUri = sourceUri;
    public FileFormatException(Uri? sourceUri, string? message, Exception? innerException) : base(message, innerException) => SourceUri = sourceUri;

    /// The file that is not in the format expected, when known.
    public Uri? SourceUri { get; }
}
