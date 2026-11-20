// GENERATED inert shells so census plug-ins LOAD (no wasm missing-type abort). Shells follow the REAL base hierarchy:
// a plug-in's base-type member call (e.g. a designer's location setter, declared on the control base) must land on
// in-object storage, or the interpreter writes the base's fields past the end of a fieldless object and corrupts the
// heap. Members stay inert; no handle/pointer is ever returned.
// ponytail: real impl = the WPF-Media D2D/DWrite layer (WIC for imaging) / WinForms control via the dialog bridge.
#pragma warning disable CS0626, CS0824, CS0108, CS0114
namespace System.Windows.Forms {
  public abstract class AxHost : Control, global::System.ComponentModel.ISupportInitialize {
    protected AxHost(string p0) { }
    public object GetOcx() => default!;
    public bool Enabled { get => default!; set { } }
    public global::System.Windows.Forms.AxHost.State OcxState { get => default!; set { } }
    [global::System.Serializable]
    public class State : global::System.Runtime.Serialization.ISerializable {
      // Form resources deserialize an OcxState via the resource reader; the type must be [Serializable] + ISerializable
      // or it throws Serialization_NonSerType. Inert: reads/writes nothing (the OCX host isn't present).
      protected State(global::System.Runtime.Serialization.SerializationInfo info, global::System.Runtime.Serialization.StreamingContext context) { }
      public void GetObjectData(global::System.Runtime.Serialization.SerializationInfo info, global::System.Runtime.Serialization.StreamingContext context) { }
    }
    public class ConnectionPointCookie {
      public ConnectionPointCookie(object source, object sink, global::System.Type eventInterface) { }
      public void Disconnect() { }
    }
    public sealed class TypeLibraryTimeStampAttribute : global::System.Attribute {
      private readonly string timestamp;
      public TypeLibraryTimeStampAttribute(string timestamp) { this.timestamp = timestamp; }
      public global::System.DateTime Value => global::System.DateTime.TryParse(timestamp, out var d) ? d : default;
    }
  }
  public class ProgressBarRenderer {
    public static void DrawHorizontalBar(global::System.Drawing.Graphics p0, global::System.Drawing.Rectangle p1) { }
  }
  public class ToolStripSeparatorRenderEventArgs : ToolStripItemRenderEventArgs {
    public ToolStripSeparatorRenderEventArgs(global::System.Drawing.Graphics g, ToolStripSeparator separator, bool vertical) : base(g, separator) { }
  }
  public class PropertyGrid : ContainerControl {
    public PropertyGrid() { }
    public bool HelpVisible { get => default!; set { } }
    public event global::System.Windows.Forms.PropertyValueChangedEventHandler PropertyValueChanged { add { } remove { } }
    public object SelectedObject { get => default!; set { } }
  }
  public class PropertyValueChangedEventArgs : global::System.EventArgs {
  }
  public class TabControlCancelEventArgs : global::System.ComponentModel.CancelEventArgs {
  }
  public delegate void PropertyValueChangedEventHandler(object s, global::System.Windows.Forms.PropertyValueChangedEventArgs e);
  public class WebBrowserBase : Control {
    public void DrawToBitmap(global::System.Drawing.Bitmap p0, global::System.Drawing.Rectangle p1) { }
  }
  public class WebBrowser : WebBrowserBase {
    public WebBrowser() { }
    public bool ScrollBarsEnabled { get => default!; set { } }
    public event global::System.Windows.Forms.WebBrowserDocumentCompletedEventHandler DocumentCompleted { add { } remove { } }
    public string DocumentText { get => default!; set { } }
    public global::System.Windows.Forms.HtmlDocument Document { get => default!; set { } }
    public global::System.Uri Url { get => default!; set { } }
    public void Refresh(global::System.Windows.Forms.WebBrowserRefreshOption p0) { }
  }
  public class WebBrowserDocumentCompletedEventArgs : global::System.EventArgs {
  }
  public delegate void WebBrowserDocumentCompletedEventHandler(object sender, global::System.Windows.Forms.WebBrowserDocumentCompletedEventArgs e);
  public sealed class HtmlDocument {
    public global::System.Windows.Forms.HtmlWindow Window { get => default!; set { } }
  }
  public sealed class HtmlWindow {
    public global::System.Drawing.Size Size { get => default!; set { } }
  }
  public enum WebBrowserRefreshOption : int { Normal = 0, IfExpired = 1, Continue = 2, Completely = 3 }
  public class HelpProvider : global::System.ComponentModel.Component {
    public HelpProvider() { }
  }
}
