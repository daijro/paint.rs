// Paint.NET's SystemLayer: what plug-ins' dialogs use from it (Ed Harvey's lay themselves out with UIUtil). The host
// runs at 96 DPI, so scaling a 96-DPI size leaves it as it is.
namespace PaintDotNet.SystemLayer;

public static class UIUtil
{
    public static int ScaleWidth(int width) => width;
    public static int ScaleHeight(int height) => height;
    public static float ScaleWidth(float width) => width;
    public static float ScaleHeight(float height) => height;
}
