// Inert stand-in for CsWinRT's WinRT.Runtime. CodeLab 6.x's editor UI (its UIUtil and ThemeUtil) is built on the
// Windows Runtime projection; the browser host has no Windows Runtime, so these types exist only so those editor
// types load and CodeLab's effect enumerates and renders. The members accept calls and do nothing (paint.rs draws
// the editor through Direct2DControl). Nothing here performs real WinRT activation. Only the surface CodeLab
// references is present.
using System;
using System.Collections.Concurrent;
using System.Runtime.InteropServices;

namespace WinRT
{
    [AttributeUsage(AttributeTargets.All, AllowMultiple = true, Inherited = false)]
    public sealed class WindowsRuntimeTypeAttribute : Attribute
    {
        public WindowsRuntimeTypeAttribute(string? sourceAssembly = null) { }
        public string? GuidSignature => null;
    }

    [AttributeUsage(AttributeTargets.All)]
    public sealed class WindowsRuntimeHelperTypeAttribute : Attribute
    {
        public WindowsRuntimeHelperTypeAttribute(Type helperType) => HelperType = helperType;
        public Type HelperType { get; }
    }

    [AttributeUsage(AttributeTargets.Class)]
    public abstract class WinRTImplementationTypeRcwFactoryAttribute : Attribute
    {
        public abstract object? CreateInstance(IInspectable inspectable);
    }

    [AttributeUsage(AttributeTargets.All)]
    public sealed class ProjectedRuntimeClassAttribute : Attribute
    {
        public ProjectedRuntimeClassAttribute(Type defaultInterface) => DefaultInterface = defaultInterface;
        public ProjectedRuntimeClassAttribute(string defaultInterfaceProperty) => DefaultInterfaceProperty = defaultInterfaceProperty;
        public Type? DefaultInterface { get; }
        public string? DefaultInterfaceProperty { get; }
    }

    [AttributeUsage(AttributeTargets.Class)]
    public sealed class WinRTExposedTypeAttribute : Attribute
    {
        public WinRTExposedTypeAttribute() { }
        public WinRTExposedTypeAttribute(Type winrtExposedTypeDetails) { }
        public ComWrappers.ComInterfaceEntry[] GetExposedInterfaces() => Array.Empty<ComWrappers.ComInterfaceEntry>();
    }

    /// A reference to a native WinRT object. Here it holds nothing and its pointer is null.
    public class IObjectReference : IDisposable
    {
        public nint ThisPtr => 0;
        public bool IsInCurrentContext => true;
        public IObjectReference As(Guid iid) => this;
        public ObjectReference<T> As<T>() => new ObjectReference<T>();
        public ObjectReference<T> As<T>(Guid iid) => new ObjectReference<T>();
        public T AsInterface<T>() => default!;
        public void Dispose() { }
    }

    public sealed class ObjectReference<T> : IObjectReference { }

    public readonly struct ObjectReferenceValue
    {
        public ObjectReferenceValue(nint ptr) { }
        public static ObjectReferenceValue Attach(ref nint ptr) => new ObjectReferenceValue(ptr);
        public nint GetAbi() => 0;
        public nint Detach() => 0;
        public void Dispose() { }
    }

    public interface IWinRTObject
    {
        IObjectReference NativeObject { get; }
        bool HasUnwrappableNativeObject { get; }
        ConcurrentDictionary<RuntimeTypeHandle, IObjectReference> QueryInterfaceCache { get; }
        ConcurrentDictionary<RuntimeTypeHandle, object> AdditionalTypeData { get; }
        IObjectReference? GetObjectReferenceForType(RuntimeTypeHandle type) => null;
    }

    public class IInspectable : IWinRTObject
    {
        public IObjectReference ObjRef => new IObjectReference();
        public IObjectReference NativeObject => ObjRef;
        public bool HasUnwrappableNativeObject => false;
        public ConcurrentDictionary<RuntimeTypeHandle, IObjectReference> QueryInterfaceCache { get; } = new();
        public ConcurrentDictionary<RuntimeTypeHandle, object> AdditionalTypeData { get; } = new();
    }

    public sealed class DerivedComposed
    {
        public static readonly DerivedComposed Instance = new DerivedComposed();
        private DerivedComposed() { }
    }

    public static class ExceptionHelpers
    {
        public static void ThrowExceptionForHR(int hr) { }
        public static int GetHRForException(Exception ex) => 0;
        public static void SetErrorInfo(Exception ex) { }
    }

    public static class ComWrappersSupport
    {
        public static object? FindObject(nint ptr) => null;
        public static T FindObject<T>(nint ptr) where T : class => null!;
        public static ObjectReference<T> GetObjectReferenceForInterface<T>(nint ptr, Guid iid) => new ObjectReference<T>();
        public static IObjectReference GetObjectReferenceForInterface(nint ptr, Guid iid, bool requireQI) => new IObjectReference();
        public static nint AllocateVtableMemory(Type type, int size) => 0;
        public static void RegisterObjectForInterface(object obj, nint thisPtr) { }
        public static object TryRegisterObjectForInterface(object obj, nint thisPtr) => obj;
        // Projection registration: CodeLab's generated WinRT projection calls these as it initializes. All no-ops here.
        public static void RegisterProjectionAssembly(System.Reflection.Assembly assembly) { }
        public static void RegisterProjectionTypeBaseTypeMapping(System.Collections.Generic.IDictionary<string, string> mapping) { }
        public static void RegisterHelperType(Type projectionType, Type helperType) { }
        public static bool RegisterDelegateFactory(Type implementationType, Func<nint, object> factory) => true;
        public static void RegisterComInterfaceEntries(Type implementationType, ComWrappers.ComInterfaceEntry[] entries) { }
    }

    public static class ComWrappersHelper
    {
        public static void Init(ref bool initialized, object obj, nint thisPtr, nint newThisPtr, Guid iid, out IObjectReference objRef)
            => objRef = new IObjectReference();
        public static void Init(IObjectReference objRef, bool addRefFromTrackerSource) { }
    }

    public static class GuidGenerator
    {
        public static Guid CreateIID(Type type) => Guid.Empty;
    }

    public sealed class ActivationFactory
    {
        public static IObjectReference Get(string typeName) => new IObjectReference();
        public static IObjectReference Get(string typeName, Guid iid) => new IObjectReference();
    }

    public struct MarshalInspectable<T> { }
    public struct Marshaler<T> { }
    public struct MarshalInterface<T> { }
    public static class MarshalDelegate
    {
        public static ObjectReferenceValue CreateMarshaler2<T>(T del, Guid iid) => default;
        public static ObjectReferenceValue CreateMarshaler2(object del, Guid iid, bool registerForMarshaling) => default;
    }
    public struct DelegateTypeDetails<T> { }

    public static class ProjectionInitializer
    {
        public static void InitializeProjection() { }
    }

    public static class ProjectionTypesInitializer
    {
        public static void InitializeProjectionTypes() { }
        public static System.Collections.Generic.Dictionary<string, string> TypeNameToBaseTypeNameMapping => new();
    }
}

namespace WinRT.Interop
{
    public struct IUnknownVftbl
    {
        public static IUnknownVftbl AbiToProjectionVftbl => default;
    }

    public interface IDisplayInformationStaticsInterop
    {
        IObjectReference GetForWindow(nint window, Guid iid) => new IObjectReference();
        IObjectReference GetForMonitor(nint monitor, Guid iid) => new IObjectReference();
    }
}

namespace ABI.WinRT.Interop
{
    public sealed class EventSource<T> where T : class
    {
        public EventSource(global::WinRT.IObjectReference objRef, int index) { }
    }

    public struct EventSourceState<T> where T : class { }

    public static class IActivationFactoryMethods
    {
        public static nint AbiToProjectionVftablePtr => 0;
        public static global::WinRT.IObjectReference ActivateInstanceUnsafe(global::WinRT.IObjectReference objRef) => new global::WinRT.IObjectReference();
        public static global::WinRT.IObjectReference ActivateInstanceUnsafe(global::WinRT.IObjectReference objRef, Guid iid) => new global::WinRT.IObjectReference();
    }
}
