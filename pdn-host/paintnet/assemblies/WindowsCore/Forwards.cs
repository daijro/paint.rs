// CodeLab 6.x (built against Paint.NET 5.2) references these types from PaintDotNet.Windows.Core, but our stand-in
// keeps them in the assemblies that own them: ManagedColor / ManagedColorExtensions in PaintDotNet.Core, and
// FontFamilyExtensions in PaintDotNet.Windows. Forward them from here so the plug-in's references resolve.
using System.Runtime.CompilerServices;

[assembly: TypeForwardedTo(typeof(PaintDotNet.Imaging.ManagedColor))]
[assembly: TypeForwardedTo(typeof(PaintDotNet.Imaging.ManagedColorExtensions))]
[assembly: TypeForwardedTo(typeof(PaintDotNet.DirectWrite.FontFamilyExtensions))]
// HeicFileTypePlus binds the public Direct2DFactory accessor in PaintDotNet.Windows.Core; it lives in PaintDotNet.Windows.
[assembly: TypeForwardedTo(typeof(PaintDotNet.Direct2D1.Direct2DFactory))]
