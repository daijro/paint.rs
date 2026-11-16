// A plug-in's own Direct2D effect, as Paint.NET 5 hosts one (PaintDotNet.Direct2D1.Effects in PaintDotNet.Windows.Core):
// the plug-in's CustomEffectProperties says which properties and inputs it has and makes its CustomEffectImpl; the impl
// builds a transform graph of effects in OnInitialize and adjusts them in OnPrepareForRender. Here the graph is not a
// Direct2D transform graph: each node is the effect it was made from, and the graph's connections set those effects'
// inputs, ending in a passthrough output, as the composed effects (ComposedEffect) are drawn. Properties keep the values
// set on them, which the impl reads back through its own Properties.
using System;
using System.Collections.Generic;
using PaintDotNet.ComponentModel;
using PaintDotNet.Interop;
using PaintDotNet.Rendering;

namespace PaintDotNet.Direct2D1.Effects
{
    public abstract class CustomEffectProperties : DeviceEffectProperties
    {
        protected CustomEffectProperties() { }

        /// The class the effect registers as (none needed here: it is never created by ID).
        protected virtual DeviceEffectID ClassID => default;

        /// The plug-in's implementation of this effect.
        protected internal abstract CustomEffectImpl CreateImpl();

        protected internal virtual string GetDisplayName() => GetType().Name;
        protected internal virtual string GetAuthor() => "";
        protected internal virtual string GetCategory() => "";
        protected internal virtual string GetDescription() => "";
        protected internal virtual EffectRegistrationType GetRegistrationType() => EffectRegistrationType.Effect;
        protected internal virtual int? GetMinInputs() => null;
        protected internal virtual int? GetMaxInputs() => null;
    }

    public abstract class CustomEffectImpl : RefTrackedObject, IDeviceEffectImpl
    {
        internal CustomEffectProperties? BoundProperties;
        private IDeviceEffectContext? effectContext;
        private ITransformGraph? transformGraph;

        /// The effect's properties, with the values set on the effect.
        protected internal CustomEffectProperties Properties => BoundProperties ?? throw new InvalidOperationException("the effect is not initialized");
        protected IDeviceEffectContext EffectContext => effectContext ?? throw new InvalidOperationException("the effect is not initialized");
        protected ITransformGraph TransformGraph => transformGraph ?? throw new InvalidOperationException("the effect is not initialized");

        object IObjectRef.Value => this;

        public virtual void Initialize(IDeviceEffectContext effectContext, ITransformGraph transformGraph)
        {
            this.effectContext = effectContext;
            this.transformGraph = transformGraph;
            OnInitialize();
        }

        protected virtual void OnInitialize() { }

        public virtual void PrepareForRender(ChangeType changeType) => OnPrepareForRender(changeType);

        protected virtual void OnPrepareForRender(ChangeType changeType) { }

        public virtual void SetGraph(ITransformGraph transformGraph)
        {
            this.transformGraph = transformGraph;
            OnSetGraph();
        }

        protected virtual void OnSetGraph() { }

        protected override void Dispose(bool disposing) { }
    }

    public abstract class CustomEffectImpl<TProperties> : CustomEffectImpl where TProperties : CustomEffectProperties, new()
    {
        protected CustomEffectImpl() { }

        protected internal new TProperties Properties => (TProperties)base.Properties;
    }

    public abstract class CustomEffect<TProperties> : DeviceEffect<TProperties>, IPropertySource where TProperties : CustomEffectProperties, new()
    {
        private readonly Dictionary<int, object?> values = new();
        private readonly CustomEffectImpl impl;
        private readonly Graph graph;
        private bool initialized;

        protected CustomEffect(IDeviceEffectFactory factory) : base(factory, 0)
        {
            graph = new Graph(this, factory);
            InputCount = Properties.GetMinInputs() ?? Properties.GetMaxInputs() ?? 1;
            impl = Properties.CreateImpl();
            impl.BoundProperties = Properties;
            impl.Initialize(new Context(factory), graph);
            initialized = true;
            Prepare(ChangeType.Graph);
        }

        protected sealed override IDeviceImage Output => graph.Output;

        protected sealed override void OnSetInput(int index, IDeviceImage? input)
        {
            graph.SetEffectInput(index, input);
            Prepare(ChangeType.Graph);
        }

        protected sealed override void OnSetInputCount(int count) => graph.EffectInputCount = count;

        protected sealed override void OnSetProperty<T>(int index, in T value)
        {
            values[index] = value;
            Prepare(ChangeType.Properties);
        }

        protected sealed override void OnSetPropertyBytes(int index, DeviceEffectPropertyType type, ReadOnlySpan<byte> value)
        {
            values[index] = value.ToArray();
            Prepare(ChangeType.Properties);
        }

        /// The impl adjusts its graph whenever the effect changes, as Direct2D asks it to before drawing.
        private void Prepare(ChangeType change)
        {
            if (initialized) impl.PrepareForRender(change);
        }

        bool IPropertySource.TryGetProperty<T>(int index, out T value)
        {
            if (values.TryGetValue(index, out object? v) && v is T t) { value = t; return true; }
            value = default!;
            return false;
        }

        protected override void Dispose(bool disposing)
        {
            if (disposing)
            {
                impl.Dispose();
                graph.Dispose();
            }
            base.Dispose(disposing);
        }

        /// A node: a reference to the effect it was made from, which the impl may dispose once the node is made.
        private sealed class Node(IDeviceEffect effect) : RefTrackedObject, ITransformNode
        {
            private readonly IDeviceEffect effect = effect.CreateRef<IDeviceEffect>();
            public IDeviceEffect Effect => effect;
            public int InputCount => effect is DeviceEffect d ? d.InputCount : effect is NativeEffect n ? n.InputCount : 1;
            object IObjectRef.Value => this;
            protected override void Dispose(bool disposing) => effect.Dispose();
        }

        private sealed class Context(IDeviceEffectFactory factory) : RefTrackedObject, IDeviceEffectContext, IFactoryProperty
        {
            public IDirect2DFactory Factory => ((IFactoryProperty)factory).Factory;
            public Vector2Float Dpi => new(96, 96);
            public IDeviceEffect CreateEffect(in DeviceEffectID effectID) => factory.CreateEffect(effectID);
            public ITransformNode CreateTransformNodeFromEffect(IDeviceEffect effect) => new Node(effect);
            public bool IsBufferPrecisionSupported(BufferPrecision precision) => true;
            public ILookupTable3D CreateLookupTable3D(BufferPrecision precision, int extentX, int extentY, int extentZ, ExtentPtr<byte> data, uint strideY, uint strideZ)
                => ((ILookupTable3DFactory)factory).CreateLookupTable3D(precision, extentX, extentY, extentZ, data, strideY, strideZ);
            object IObjectRef.Value => this;
            protected override void Dispose(bool disposing) { }
        }

        /// The impl's wiring, applied to the nodes' effects: effect inputs into nodes, nodes into nodes, a node (or an
        /// effect input, when the graph passes one through) into the output.
        private sealed class Graph : RefTrackedObject, ITransformGraph
        {
            private readonly CustomEffect<TProperties> owner;
            private readonly IDeviceEffect output;
            private readonly List<IDeviceImage?> inputs = new();
            private readonly List<(int Input, Node Node, int NodeInput)> fromInputs = new();
            private readonly List<Node> nodes = new(); // the graph's references to its nodes, which the impl may dispose
            private int passthrough = -1;

            public Graph(CustomEffect<TProperties> owner, IDeviceEffectFactory factory)
            {
                this.owner = owner;
                output = factory.CreateEffect(Clsid.D2D1Composite);
                ((NativeEffect)output).SetInputCount(1);
            }

            public IDeviceImage Output => output;
            public int EffectInputCount { get; set; }
            public int InputCount => owner.InputCount;
            object IObjectRef.Value => this;

            public void SetEffectInput(int index, IDeviceImage? input)
            {
                while (inputs.Count <= index) inputs.Add(null);
                inputs[index] = input;
                foreach (var c in fromInputs)
                    if (c.Input == index) c.Node.Effect.SetInput(c.NodeInput, input);
                if (passthrough == index) output.SetInput(0, input);
            }

            private IDeviceImage? EffectInput(int index) => index < inputs.Count ? inputs[index] : null;

            public void SetSingleTransformNode(ITransformNode node)
            {
                Clear();
                AddNode(node);
                var n = (Node)node;
                for (int i = 0; i < n.InputCount; i++) ConnectToEffectInput(i, node, i);
                SetOutputNode(node);
            }

            public void AddNode(ITransformNode node)
            {
                if (!nodes.Contains((Node)node)) nodes.Add(node.CreateRef<Node>());
            }

            public void RemoveNode(ITransformNode node)
            {
                fromInputs.RemoveAll(c => c.Node == node);
                if (nodes.Remove((Node)node)) node.Dispose();
            }

            public void SetOutputNode(ITransformNode node)
            {
                passthrough = -1;
                output.SetInput(0, ((Node)node).Effect);
            }

            public void ConnectNode(ITransformNode fromNode, ITransformNode toNode, int toNodeInputIndex)
                => ((Node)toNode).Effect.SetInput(toNodeInputIndex, ((Node)fromNode).Effect);

            public void ConnectToEffectInput(int toEffectInputIndex, ITransformNode node, int toNodeInputIndex)
            {
                var n = (Node)node;
                fromInputs.RemoveAll(c => c.Node == n && c.NodeInput == toNodeInputIndex);
                fromInputs.Add((toEffectInputIndex, n, toNodeInputIndex));
                n.Effect.SetInput(toNodeInputIndex, EffectInput(toEffectInputIndex));
            }

            public void Clear()
            {
                fromInputs.Clear();
                passthrough = -1;
                output.SetInput(0, null);
                foreach (var n in nodes) n.Dispose();
                nodes.Clear();
            }

            public void SetPassthroughGraph(int effectInputIndex)
            {
                Clear();
                passthrough = effectInputIndex;
                output.SetInput(0, EffectInput(effectInputIndex));
            }

            protected override void Dispose(bool disposing)
            {
                Clear();
                output.Dispose();
            }
        }
    }

    public static class EffectPropertyAccessorExtensions
    {
        /// A list property's values.
        public static void SetValue<T>(this EffectPropertyAccessor<IReadOnlyList<T>> accessor, ReadOnlySpan<T> values) where T : unmanaged
            => accessor.SetValue(values.ToArray());
    }
}
