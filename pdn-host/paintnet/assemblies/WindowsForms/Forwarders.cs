// Padding and Message live in System.Windows.Forms.Primitives (as in .NET 7+ WinForms).
using System.Runtime.CompilerServices;

[assembly: TypeForwardedTo(typeof(System.Windows.Forms.Padding))]
[assembly: TypeForwardedTo(typeof(System.Windows.Forms.Message))]
