// The design-time editors and designer the library's [Editor]/[Designer] attributes name (upstream's
// Design/, left out: a headless host has no Visual Studio property grid). Empty, so the attributes
// resolve when the serializer or TypeDescriptor reads them.
using System.Drawing.Design;

namespace System.Windows.Forms.Design.DataVisualization.Charting
{
    internal class AnchorPointUITypeEditor : UITypeEditor { }
    internal class AnnotationAxisUITypeEditor : UITypeEditor { }
    internal class AnnotationCollectionEditor : UITypeEditor { }
    internal class AxesArrayEditor : UITypeEditor { }
    internal class ChartCollectionEditor : UITypeEditor { }
    internal class ChartColorEditor : UITypeEditor { }
    internal class ChartTypeEditor : UITypeEditor { }
    internal class ColorPaletteEditor : UITypeEditor { }
    internal class DataPointCollectionEditor : UITypeEditor { }
    internal class FlagsEnumUITypeEditor : UITypeEditor { }
    internal class GradientEditor : UITypeEditor { }
    internal class HatchStyleEditor : UITypeEditor { }
    internal class ImageValueEditor : UITypeEditor { }
    internal class KeywordsStringEditor : UITypeEditor { }
    internal class LegendCellCollectionEditor : UITypeEditor { }
    internal class LegendCellColumnCollectionEditor : UITypeEditor { }
    internal class LegendCollectionEditor : UITypeEditor { }
    internal class LegendItemCollectionEditor : UITypeEditor { }
    internal class MarkerStyleEditor : UITypeEditor { }
    internal class SeriesCollectionEditor : UITypeEditor { }
    internal class SeriesDataSourceMemberValueAxisUITypeEditor : UITypeEditor { }
    internal class ChartWinDesigner { }
    internal class ChartWinDesignerSerializer : System.ComponentModel.Design.Serialization.CodeDomSerializer { }
}

namespace System.ComponentModel.Design.Serialization
{
    // The Windows Forms designer's code serializer (System.Windows.Forms.Design), named by Chart's
    // [DesignerSerializer]; only the designer ever instantiates it.
    internal class CodeDomSerializer { }
}
