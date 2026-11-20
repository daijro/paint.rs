// Effect-document metadata interfaces the survey corpus names. The host surfaces no live document metadata, so these
// are the interface shapes a plug-in reads; shapes are the reference's public metadata.
namespace PaintDotNet.Effects
{
    public interface IBitmapEffectLayerInfo : IEffectLayerInfo, PaintDotNet.ComponentModel.IObjectRef, System.IDisposable, PaintDotNet.IIsDisposed, PaintDotNet.ComponentModel.IInternalImpl
    {
    }

    public interface IEffectDocumentMetadata : PaintDotNet.ComponentModel.IObjectRef, System.IDisposable, PaintDotNet.IIsDisposed, PaintDotNet.ComponentModel.IInternalImpl
    {
        System.Collections.Generic.IReadOnlyList<PaintDotNet.Imaging.ExifPropertyItem> ExifPropertyItems { get; }
        System.Collections.Generic.IReadOnlyList<PaintDotNet.Imaging.IptcPropertyItem> IptcPropertyItems { get; }
        PaintDotNet.Imaging.XmpPacket XmpPacket { get; }
    }
}
