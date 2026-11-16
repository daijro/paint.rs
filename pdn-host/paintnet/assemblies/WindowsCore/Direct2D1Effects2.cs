// More Direct2D built-ins as Paint.NET wraps them (PaintDotNet.Direct2D1.Effects in PaintDotNet.Windows.Core): Border,
// ConvolveMatrix and Scale, with the convenience constructors plug-ins call. Property indices are Direct2D's.
using System;
using System.Collections.Generic;
using System.Linq;
using PaintDotNet.Interop;
using PaintDotNet.Rendering;

namespace PaintDotNet.Direct2D1.Effects
{
    /// An effect that measures its input (DeviceContextExtensions.RunAnalysisEffect runs it) rather than drawing.
    public interface IAnalysisEffect : IDeviceEffect { }

    /// How an analysis effect here measures: over what `dc` renders of its input.
    internal interface IAnalysisRunner
    {
        void RunAnalysis(IDeviceContext dc);
    }

    /// An effect whose properties can be read back (an analysis's output).
    internal interface IPropertySource
    {
        bool TryGetProperty<T>(int index, out T value);
    }

    public sealed class BorderEffect : DeviceEffect<BorderEffect.Props>
    {
        public BorderEffect(IDeviceEffectFactory factory) : base(factory, Clsid.D2D1Border) { }
        public BorderEffect(IDeviceEffectFactory factory, IDeviceImage? input, BorderEdgeMode edgeMode) : this(factory)
        {
            Properties.Input.Set(input);
            Properties.EdgeModeX.SetValue(edgeMode);
            Properties.EdgeModeY.SetValue(edgeMode);
        }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input => GetInput(0);
            public EffectPropertyAccessor<BorderEdgeMode> EdgeModeX => GetProperty<BorderEdgeMode>(0, DeviceEffectPropertyType.Enum);
            public EffectPropertyAccessor<BorderEdgeMode> EdgeModeY => GetProperty<BorderEdgeMode>(1, DeviceEffectPropertyType.Enum);
        }
    }

    public sealed class ConvolveMatrixEffect : DeviceEffect<ConvolveMatrixEffect.Props>
    {
        public ConvolveMatrixEffect(IDeviceEffectFactory factory) : base(factory, Clsid.D2D1ConvolveMatrix) { }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input => GetInput(0);
            public EffectPropertyAccessor<float> KernelUnitLength => GetProperty<float>(0, DeviceEffectPropertyType.Float);
            public EffectPropertyAccessor<uint> KernelSizeX => GetProperty<uint>(2, DeviceEffectPropertyType.UInt32);
            public EffectPropertyAccessor<uint> KernelSizeY => GetProperty<uint>(3, DeviceEffectPropertyType.UInt32);
            public EffectPropertyAccessor<float> Divisor => GetProperty<float>(5, DeviceEffectPropertyType.Float);
            public EffectPropertyAccessor<float> Bias => GetProperty<float>(6, DeviceEffectPropertyType.Float);
            public EffectPropertyAccessor<Vector2Float> KernelOffset => GetProperty<Vector2Float>(7, DeviceEffectPropertyType.Vector2Float);
            public EffectPropertyAccessor<bool> PreserveAlpha => GetProperty<bool>(8, DeviceEffectPropertyType.Boolean);
            public EffectPropertyAccessor<BorderMode> BorderMode => GetProperty<BorderMode>(9, DeviceEffectPropertyType.Enum);
            public EffectPropertyAccessor<bool> ClampOutput => GetProperty<bool>(10, DeviceEffectPropertyType.Boolean);

            /// The kernel as rows of weights: sets its size and its values (row by row).
            public void SetKernelMatrixAndSize(IReadOnlyList<IReadOnlyList<float>> kernel)
            {
                int rows = kernel.Count, cols = rows == 0 ? 0 : kernel[0].Count;
                if (kernel.Any(r => r.Count != cols)) throw new ArgumentException("the kernel's rows must be the same length", nameof(kernel));
                KernelSizeX.SetValue((uint)cols);
                KernelSizeY.SetValue((uint)rows);
                float[] values = kernel.SelectMany(r => r).ToArray();
                ((IPropertySink)Effect).SetPropertyBytes(4, DeviceEffectPropertyType.Blob, System.Runtime.InteropServices.MemoryMarshal.AsBytes(values.AsSpan()));
            }
        }
    }

    public sealed class ScaleEffect : DeviceEffect<ScaleEffect.Props>
    {
        public ScaleEffect(IDeviceEffectFactory factory) : base(factory, Clsid.D2D1Scale) { }
        public ScaleEffect(IDeviceEffectFactory factory, IDeviceImage? input, Vector2Float scale, Point2Float centerPoint = default,
                           ScaleInterpolationMode interpolationMode = ScaleInterpolationMode.Linear, BorderMode borderMode = BorderMode.Soft,
                           float sharpness = 0f) : this(factory)
        {
            Properties.Input.Set(input);
            Properties.Scale.SetValue(scale);
            Properties.CenterPoint.SetValue(new Vector2Float(centerPoint.X, centerPoint.Y));
            Properties.InterpolationMode.SetValue(interpolationMode);
            Properties.BorderMode.SetValue(borderMode);
            Properties.Sharpness.SetValue(sharpness);
        }

        public sealed class Props : DeviceEffectProperties
        {
            public EffectInputAccessor Input => GetInput(0);
            public EffectPropertyAccessor<Vector2Float> Scale => GetProperty<Vector2Float>(0, DeviceEffectPropertyType.Vector2Float);
            public EffectPropertyAccessor<Vector2Float> CenterPoint => GetProperty<Vector2Float>(1, DeviceEffectPropertyType.Vector2Float);
            public EffectPropertyAccessor<ScaleInterpolationMode> InterpolationMode => GetProperty<ScaleInterpolationMode>(2, DeviceEffectPropertyType.Enum);
            public EffectPropertyAccessor<BorderMode> BorderMode => GetProperty<BorderMode>(3, DeviceEffectPropertyType.Enum);
            public EffectPropertyAccessor<float> Sharpness => GetProperty<float>(4, DeviceEffectPropertyType.Float);
        }
    }
}
