# System.Windows.Forms.DataVisualization in paint.rs

Microsoft's chart control for Windows Forms, vendored from https://github.com/dotnet/winforms-datavisualization
at commit `022f396f0561532c58d3af6f16cba8daa4386ce6` (`src/System.Windows.Forms.DataVisualization`,
the source of the 1.0.0 NuGet package plugins reference). MIT License (`LICENSE.TXT`, `NOTICE.md`);
`THIRD-PARTY-NOTICES.TXT` is upstream's.

The .NET plugin host builds it against its headless `System.Windows.Forms` stand-in
(`../WindowsForms`) and the host's System.Drawing, so a plugin that draws a `Chart` off-screen
(BoltBait's Chart or Graph: `Chart.Serializer.Load`, then `Chart.SaveImage`) renders as the library
draws it, on libgdiplus off Windows.

Changes from upstream:

- Left out: the Visual Studio designer (`Design/`, `Annotation/AnnotationDesign.cs`,
  `Common/EditorNames.cs`, which upstream's project also leaves out), the code-analysis suppressions
  (`FxCopExclusionsByDesign.cs`), the designer assembly's `InternalsVisibleTo`, the localised
  `xlf/` sources and `Resources/` (designer bitmaps and strings). `DesignStubs.cs` (paint.rs) names the
  editor and designer types the library's `[Editor]`/`[Designer]` attributes point at, empty.
- `upstream/Design/AxesArrayDesigner.cs` keeps only `DataPointCustomPropertiesConverter` and
  `DataPointConverter`, the type converters `DataPoint` and `DataPointCustomProperties` name; the
  axes array editor is removed.
- `General/Chart.cs`: data binding tests for `System.Data.Common`'s `DbCommand`, `DbDataAdapter` and
  `DbDataReader` instead of SqlClient's subclasses of them (the same members, without shipping
  System.Data.SqlClient), and the EMF export no longer demands `SecurityPermission` (code access
  security does nothing on .NET Core).
- Built by `WindowsFormsDataVisualization.csproj` (paint.rs) with upstream's `WINFORMS_CONTROL`
  define and its `SR.resx` embedded under upstream's resource name.
