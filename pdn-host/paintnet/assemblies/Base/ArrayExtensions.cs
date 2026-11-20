// The WebP file types bind this in PaintDotNet.Base. ponytail: CloneT shallow-clones the array, which is all the
// corpus uses it for; real = genuine's element-aware clone if a plug-in ever needs deep copies.
namespace PaintDotNet;

public static class ArrayExtensions
{
    public static T[] CloneT<T>(this T[] array) => (T[])array.Clone();
}
