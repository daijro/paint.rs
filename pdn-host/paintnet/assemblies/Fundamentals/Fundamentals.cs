using System;

namespace PaintDotNet
{
    public sealed class ProgressEventArgs : EventArgs
    {
        public ProgressEventArgs(double percent) : this(percent, clamp: true) { }
        public ProgressEventArgs(double percent, bool clamp)
        {
            if (!double.IsFinite(percent))
                throw new ArgumentException($"percent must be a finite value ({percent})", nameof(percent));
            Percent = clamp ? Math.Clamp(percent, 0.0, 100.0) : percent;
        }
        public double Percent { get; private set; }
    }

    public delegate void ProgressEventHandler(object? sender, ProgressEventArgs e);

    public static class ServiceProviderExtensions
    {
        public static TService? GetService<TService>(this IServiceProvider serviceProvider) where TService : class
            => serviceProvider.GetService(typeof(TService)) as TService;
    }

    public static class DisposableUtil
    {
        // T is not constrained to IDisposable: the plugins free interface handles like
        // IBitmapSource<T> that are only disposable at runtime, so dispose via a cast.
        public static void Free<T>(ref T? disposeMe) where T : class
        { (disposeMe as IDisposable)?.Dispose(); disposeMe = null; }
        public static void Free<T>(ref T? disposeMe, bool callerIsNotFinalizing) where T : class
        { (disposeMe as IDisposable)?.Dispose(); disposeMe = null; }
    }

    public static class ExceptionUtil
    {
        public static void ThrowArgumentNullException() => throw new ArgumentNullException();
        public static void ThrowArgumentNullException(string paramName) => throw new ArgumentNullException(paramName);
        public static void ThrowArgumentNullException(string paramName, string message) => throw new ArgumentNullException(paramName, message);
        public static void ThrowObjectDisposedException(string objectName) => throw new ObjectDisposedException(objectName);
        public static void ThrowInvalidOperationException(string message) => throw new InvalidOperationException(message);
        public static void ThrowArgumentException(string message, string paramName) => throw new ArgumentException(message, paramName);
    }

    // 1D pointer+length view. The JPEG XL save write-callback wraps its native buffer in
    // one before handing it to the stream. Scoped to Ptr/Length (what that path reads).
    public readonly unsafe ref struct ExtentPtr<T> where T : unmanaged
    {
        private readonly T* ptr;
        public ExtentPtr(T* ptr, int length) { this.ptr = ptr; Length = length; }
        public ExtentPtr(T* ptr, long length) { this.ptr = ptr; Length = (nint)length; }
        public ExtentPtr(T* ptr, nint length) { this.ptr = ptr; Length = length; }
        public T* Ptr => ptr;
        public nint Length { get; }
        public bool IsEmpty => ptr == null || Length == 0;
    }
}

namespace PaintDotNet.IO
{
    using PaintDotNet;

    public static class StreamExtensions
    {
        public static unsafe void Write(this System.IO.Stream stream, ExtentPtr<byte> extent)
            => stream.Write(new ReadOnlySpan<byte>(extent.Ptr, checked((int)extent.Length)));

        /// Reads until <paramref name="count"/> bytes arrived or the stream ended; returns the bytes read.
        public static int ProperRead(this System.IO.Stream stream, byte[] buffer, int offset, int count)
            => stream.ReadAtLeast(buffer.AsSpan(offset, count), count, throwOnEndOfStream: false);
    }
}
