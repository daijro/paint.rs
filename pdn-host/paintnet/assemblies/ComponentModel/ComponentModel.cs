using System;
using System.Threading;

namespace PaintDotNet
{
    public interface IIsDisposed
    {
        bool IsDisposed { get; }
    }

    // Base for the plugins' small disposable helpers (stream writers, image holders).
    public abstract class Disposable : IDisposable, IIsDisposed
    {
        private int isDisposed;
        protected Disposable() { }
        public bool IsDisposed => Volatile.Read(ref isDisposed) != 0;
        public void Dispose()
        {
            if (Interlocked.Exchange(ref isDisposed, 1) == 0)
            {
                Dispose(disposing: true);
                GC.SuppressFinalize(this);
            }
        }
        protected virtual void Dispose(bool disposing) { }
    }
}

namespace PaintDotNet.ComponentModel
{
    // A ref-counted handle to a shared object. AVIF/DDS reference it; on the save
    // path they only need the type to resolve and expose the wrapped value.
    /// Marker that many Paint.NET 5 public interfaces carry (its one real member is internal, so a plug-in cannot
    /// see or implement it). Present only so those interfaces' base lists resolve when a plug-in loads.
    public interface IInternalImpl
    {
    }

    public interface IObjectRef : IDisposable
    {
        object Value { get; }
    }

    /// Genuine Paint.NET's disposable base (effects derive from it); plugins dispose the built-ins
    /// they hold through it. It is ref-counted, as Paint.NET's is: CreateRef takes another reference
    /// and the object is disposed when the last one is.
    // ponytail: a reference is the same object, not a new one, so disposing one reference twice
    // releases two; give each reference its own object if a plug-in does that.
    public class RefTrackedObject : IObjectRef, IIsDisposed
    {
        private int refs = 1;
        public RefTrackedObject() { }
        public bool IsDisposed => Volatile.Read(ref refs) <= 0;
        object IObjectRef.Value => this;
        public virtual void Dispose()
        {
            if (Interlocked.Decrement(ref refs) == 0)
            {
                Dispose(disposing: true);
                GC.SuppressFinalize(this);
            }
        }
        protected virtual void Dispose(bool disposing) { }

        /// Another reference to this object, when it is one of type `type`.
        public virtual bool? TryCreateRef(Type type, out IObjectRef? objectRef)
        {
            objectRef = null;
            if (!type.IsInstanceOfType(this)) return false;
            if (Interlocked.Increment(ref refs) <= 1) { Interlocked.Decrement(ref refs); throw new ObjectDisposedException(GetType().Name); }
            objectRef = this;
            return true;
        }
    }

    public static class ObjectRefExtensions
    {
        /// Another reference to the object, which the caller disposes; objects that are not ref-counted are returned as they are.
        public static T CreateRef<T>(this IObjectRef objectRef) where T : class, IObjectRef
            => (T)CreateRef(objectRef, typeof(T));

        public static IObjectRef CreateRef(this IObjectRef objectRef) => CreateRef(objectRef, objectRef.GetType());

        public static IObjectRef CreateRef(this IObjectRef objectRef, Type type)
            => TryCreateRef(objectRef, type) ?? throw new InvalidCastException($"{objectRef.GetType().Name} is not a {type.Name}");

        public static IObjectRef? TryCreateRef(this IObjectRef objectRef, Type type) => objectRef switch
        {
            RefTrackedObject r => r.TryCreateRef(type, out IObjectRef? o) == true ? o : null,
            IRefCounted c when type.IsInstanceOfType(objectRef) => c.AddRef(),
            _ when type.IsInstanceOfType(objectRef) => objectRef,
            _ => null,
        };
    }

    /// A ref-counted object outside RefTrackedObject (the host's COM wrappers): AddRef takes a reference and returns it.
    internal interface IRefCounted
    {
        IObjectRef AddRef();
    }
}

namespace PaintDotNet
{
    /// An invariant Paint.NET's code checks failed.
    public class InternalErrorException : Exception
    {
        public InternalErrorException() { }
        public InternalErrorException(string message) : base(message) { }
        public InternalErrorException(string message, Exception innerException) : base(message, innerException) { }
    }

    /// Something was used before it was initialised.
    public class NotInitializedException : InvalidOperationException
    {
        public NotInitializedException() { }
        public NotInitializedException(string message) : base(message) { }
    }
}
