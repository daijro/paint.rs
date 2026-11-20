// Type forwarders: PaintDotNet.Core (assembly) -> where a type actually lives in this replacement set. Pre-5.2
// plug-ins bind some types to the Core assembly identity that Paint.NET 5 split out; forward them so the plug-in's
// reference resolves to the one real type (CurtisBlack.Common's ControlShadow.GdiPaint is a Core-bound
// ISurface<ColorBgra>; ISurface moved to PaintDotNet.Primitives, where Core's own Surface already implements it).
using System.Runtime.CompilerServices;

[assembly: TypeForwardedTo(typeof(PaintDotNet.Rendering.ISurface<>))]
