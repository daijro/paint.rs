// What makes the desktop rewrite a plug-in as it loads (a network call: the gate routes it) together with what makes
// writing the rewritten module back need another assembly: a parameter default of a WinForms 2.0 enum type, whose
// underlying type Cecil reads from WinForms itself. Simon Brown's SBCommon has both. Our own code.
using System.Net;
using System.Windows.Forms;

namespace PdnResolveTest;

public static class Fetcher
{
    /// Never called: it only has to be there to be rewritten.
    public static WebRequest Fetch(string url, AnchorStyles anchor = AnchorStyles.Top | AnchorStyles.Left)
        => WebRequest.Create(url + (int)anchor);
}
