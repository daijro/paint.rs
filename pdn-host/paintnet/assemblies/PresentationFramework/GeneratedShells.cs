// GENERATED inert shells so census plug-ins LOAD (no wasm missing-type abort).
// ponytail: real impl = the WPF-Media D2D/DWrite layer (WIC for imaging) / WinForms control via dialog bridge.
#pragma warning disable CS0626, CS0824
namespace System.Windows.Shapes {
  public class Path {
    public Path() { }
    public global::System.Windows.Media.Geometry Data { get => default!; set { } }
  }
  public class Shape {
    public global::System.Windows.Media.Brush Stroke { get => default!; set { } }
    public global::System.Windows.Media.Brush Fill { get => default!; set { } }
    public double StrokeThickness { get => default!; set { } }
    public global::System.Windows.Media.Stretch Stretch { get => default!; set { } }
  }
}
namespace System.Windows.Controls {
  public class Canvas {
    public Canvas() { }
  }
  public class Panel {
    public global::System.Windows.Media.Brush Background { get => default!; set { } }
    public global::System.Windows.Controls.UIElementCollection Children { get => default!; set { } }
  }
  public class UIElementCollection {
    public int Add(global::System.Windows.UIElement p0) => default!;
  }
  public class UserControl {
    public UserControl() { }
  }
  public class ControlTemplate {
    public ControlTemplate() { }
  }
}
namespace System.Windows {
  public class FrameworkElement {
    public global::System.Windows.HorizontalAlignment HorizontalAlignment { get => default!; set { } }
    public global::System.Windows.VerticalAlignment VerticalAlignment { get => default!; set { } }
    public double Width { get => default!; set { } }
    public double Height { get => default!; set { } }
    public global::System.Windows.Thickness Margin { get => default!; set { } }
    
    
    public FrameworkElement() { }
    public global::System.Windows.Size MeasureOverride(global::System.Windows.Size p0) => default!;
    public global::System.Windows.Size ArrangeOverride(global::System.Windows.Size p0) => default!;
  }
  public enum HorizontalAlignment : int { _Shell = 0 }
  public enum VerticalAlignment : int { _Shell = 0 }
  public struct Thickness {
    public Thickness(double p0) { }
    public Thickness(double p0, double p1, double p2, double p3) { }
  }
  public class Application {
    public static global::System.Reflection.Assembly ResourceAssembly { get => default!; set { } }
    
    public static void LoadComponent(object p0, global::System.Uri p1) { }
  }
}
