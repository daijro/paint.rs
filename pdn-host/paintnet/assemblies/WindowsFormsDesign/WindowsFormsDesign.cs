// Headless stand-ins for the System.Windows.Forms.Design designer base types a plug-in's control references at load.
// CodeLab's embedded ScintillaNET defines ScintillaDesigner : ControlDesigner and calls base PreFilterProperties and
// the inherited Component. Designers never run in the host, so these only need to resolve and be inert.
using System.Collections;
using System.ComponentModel;

namespace System.ComponentModel.Design
{
    public class ComponentDesigner : IDisposable
    {
        public virtual IComponent Component => null;
        public virtual void Initialize(IComponent component) { }
        protected virtual void PreFilterProperties(IDictionary properties) { }
        protected virtual void PreFilterAttributes(IDictionary attributes) { }
        protected virtual void PostFilterProperties(IDictionary properties) { }
        protected virtual void PostFilterAttributes(IDictionary attributes) { }
        public void Dispose() { Dispose(true); }
        protected virtual void Dispose(bool disposing) { }
    }
}

namespace System.Windows.Forms.Design
{
    // ControlDesigner inherits PreFilterProperties and Component from ComponentDesigner, which is what
    // ScintillaNET's ScintillaDesigner overrides and calls.
    public class ControlDesigner : System.ComponentModel.Design.ComponentDesigner
    {
    }
}
