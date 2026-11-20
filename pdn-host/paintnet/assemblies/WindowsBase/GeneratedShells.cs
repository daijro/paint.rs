// GENERATED inert shells so census plug-ins LOAD (no wasm missing-type abort).
// ponytail: real impl = the WPF-Media D2D/DWrite layer (WIC for imaging) / WinForms control via dialog bridge.
#pragma warning disable CS0626, CS0824
namespace System.Windows {
  public class DependencyProperty {
    public static global::System.Windows.DependencyProperty Register(string p0, global::System.Type p1, global::System.Type p2, global::System.Windows.PropertyMetadata p3) => default!;
  }
  public class DependencyObject {
    public object GetValue(global::System.Windows.DependencyProperty p0) => default!;
    public void SetValue(global::System.Windows.DependencyProperty p0, object p1) { }
  }
  public struct DependencyPropertyChangedEventArgs {
  }
  public delegate void PropertyChangedCallback(global::System.Windows.DependencyObject d, global::System.Windows.DependencyPropertyChangedEventArgs e);
  public delegate object CoerceValueCallback(global::System.Windows.DependencyObject d, object baseValue);
  public class PropertyMetadata {
    public PropertyMetadata(object p0, global::System.Windows.PropertyChangedCallback p1, global::System.Windows.CoerceValueCallback p2) { }
    public PropertyMetadata(object p0) { }
    public PropertyMetadata(object p0, global::System.Windows.PropertyChangedCallback p1) { }
  }
}
