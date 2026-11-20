// Document-view support in PaintDotNet.Base that classic plug-ins reference (CurtisBlack.Common): a generic
// event-args carrier, Paint.NET's own generic WeakReference, and the Functional Try/Result pair. Our own code.
using System;

namespace PaintDotNet
{
    /// Carries one value with an event (Paint.NET's PaintDotNet.EventArgs`1).
    public class EventArgs<T> : System.EventArgs
    {
        public EventArgs(T data) => Data = data;
        public T Data { get; }
    }

    /// Paint.NET's own generic weak reference (distinct from System.WeakReference<T>).
    public class WeakReference<T> where T : class
    {
        private readonly System.WeakReference inner;
        public WeakReference(T target) => inner = new System.WeakReference(target);
        public T? Target => inner.Target as T;
    }
}

namespace PaintDotNet.Functional
{
    /// The outcome of a Func.Try: success, or the exception it caught; Observe() is the "handled it" acknowledgement.
    public class Result
    {
        public Result() { }
        public Result(Exception? error) => Error = error;
        public Exception? Error { get; }
        public void Observe() { }
    }

    public static class Func
    {
        public static Result Try(Action action)
        {
            try { action(); return new Result(); }
            catch (Exception e) { return new Result(e); }
        }
        public static Result Try<T>(Action<T> action, T arg)
        {
            try { action(arg); return new Result(); }
            catch (Exception e) { return new Result(e); }
        }
    }
}
