// Adds an AssemblyResolve handler that loads the very name it was asked for, as a plug-in with that habit does, then
// asks for an assembly nothing has: green when that fails the ordinary way (the host keeps the handler from
// recursing), red if it somehow loads. Without the host's guard the handler recurses until the stack runs out and
// the process ends. Our own code.
using System;
using System.Drawing;
using System.IO;
using System.Reflection;
using PaintDotNet;
using PaintDotNet.Effects;
using PaintDotNet.PropertySystem;

namespace PdnSelfTest;

public sealed class ResolveLoopEffect : PropertyBasedEffect
{
    public ResolveLoopEffect() : base("Resolve Loop", null, null, new EffectOptions()) { }

    protected override PropertyCollection OnCreatePropertyCollection() => PropertyCollection.CreateEmpty();

    protected override void OnRender(Rectangle[] rois, int startIndex, int length)
    {
        ResolveEventHandler again = (_, args) => Assembly.Load(args.Name);
        AppDomain.CurrentDomain.AssemblyResolve += again;
        ColorBgra colour;
        try
        {
            Assembly.Load("PdnSelfTest.Nowhere");
            colour = ColorBgra.FromBgra(0, 0, 255, 255);
        }
        catch (FileNotFoundException)
        {
            colour = ColorBgra.FromBgra(0, 255, 0, 255);
        }
        finally
        {
            AppDomain.CurrentDomain.AssemblyResolve -= again;
        }
        for (int i = startIndex; i < startIndex + length; i++) DstArgs.Surface.Fill(rois[i], colour);
    }
}
