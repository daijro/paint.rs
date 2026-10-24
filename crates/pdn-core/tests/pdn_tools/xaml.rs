use pdn_core::{
    canvas::{FillRule, Path2D},
    pixel::IntRect,
    tool::{
        shapes::{self, DashStyle, DrawType},
        xaml::*,
    },
};
const NS: &str = r#"xmlns:s="clr-namespace:PaintDotNet.Shapes;assembly=PaintDotNet.Framework" xmlns="clr-namespace:PaintDotNet.UI.Media;assembly=PaintDotNet.UI" xmlns:p="clr-namespace:PaintDotNet.Rendering;assembly=PaintDotNet.Primitives" xmlns:o="clr-namespace:PaintDotNet.ObjectModel;assembly=PaintDotNet.ObjectModel" xmlns:sys="clr-namespace:System;assembly=mscorlib""#;
fn shape(body: &str) -> ParsedShape {
    parse(&format!(
        r#"<s:SimpleGeometryShape {NS}>{body}</s:SimpleGeometryShape>"#
    ))
    .unwrap()
}
fn b(s: &ParsedShape) -> [f64; 4] {
    [s.bounds.x, s.bounds.y, s.bounds.width, s.bounds.height]
}
fn near(a: [f64; 4], b: [f64; 4]) {
    assert!(
        a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-4),
        "{a:?} != {b:?}"
    );
}
fn inside(s: &ParsedShape, x: f32, y: f32) -> bool {
    s.geometry.contains(x, y, s.fill_rule)
}
#[test]
fn xaml_markup_bounds() {
    let s = parse(&format!(
        r#"<s:SimpleGeometryShape {NS} DisplayName="Tri" Geometry="F1 M0,10 L5,0 10,10Z"/>"#
    ))
    .unwrap();
    assert_eq!(s.name, "Tri");
    assert_eq!(s.fill_rule, FillRule::NonZero);
    near(b(&s), [0., 0., 10., 10.]);
    assert_eq!(
        parse_path_markup("F0 M0 0 l5-10 5 10z").unwrap().1,
        FillRule::EvenOdd
    );
    let (p, _) = parse_path_markup("m1,1 h4 v2 h-4z").unwrap();
    let r = path_bounds(&p);
    near([r.x, r.y, r.width, r.height], [1., 1., 4., 2.]);
    let s = shape(r#"<PathGeometry Figures="M0,0 A6,6 0 0 1 10,10 C8,14 2,14 0,10 Q-2,5 0,0Z"/>"#);
    let r = 50f64.sqrt();
    near(b(&s), [-1., 5. - r, 6. + r, 8. + r]);
}
#[test]
fn xaml_transformed_ellipse_and_spline() {
    let s = shape(
        r#"<EllipseGeometry RadiusX="2" RadiusY="1"><EllipseGeometry.Transform><RotateTransform Angle="45"/></EllipseGeometry.Transform></EllipseGeometry>"#,
    );
    let e = 2.5f64.sqrt();
    near(b(&s), [-e, -e, 2. * e, 2. * e]);
    let s = shape(
        r#"<PathGeometry><PathFigure><PolyCurveSegment><PolyCurveSegment.Points><p:Point2Double X="10" Y="10"/><p:Point2Double X="20"/><p:Point2Double X="30" Y="10"/></PolyCurveSegment.Points></PolyCurveSegment></PathFigure></PathGeometry>"#,
    );
    assert_eq!(s.name, "SimpleGeometryShapeWrapper");
    let mut p = Path2D::new();
    p.move_to(0., 0.);
    for [a, b, c] in shapes::natural_spline(&[[0., 0.], [10., 10.], [20., 0.], [30., 10.]]) {
        p.bezier_curve_to(a[0], a[1], b[0], b[1], c[0], c[1]);
    }
    let r = path_bounds(&p);
    near(b(&s), [r.x, r.y, r.width, r.height]);
}
#[test]
fn xaml_fill_flags_outline_and_widen_order() {
    let hollow = r#"<PathGeometry><PathFigure IsFilled="False" IsClosed="True"><LineSegment Point="10,0"/><LineSegment Point="10,10"/><LineSegment Point="0,10"/></PathFigure></PathGeometry>"#;
    let s = shape(&format!("<OutlinedGeometry>{hollow}</OutlinedGeometry>"));
    assert!(!inside(&s, 5., 5.));
    let s = shape(&format!(
        r#"<WidenedGeometry Thickness="2">{hollow}</WidenedGeometry>"#
    ));
    near(b(&s), [-1., -1., 12., 12.]);
    assert!(!inside(&s, 5., 5.));
    let s = shape(r#"<OutlinedGeometry><RectangleGeometry Rect="0,0,10,10"/></OutlinedGeometry>"#);
    near(b(&s), [0., 0., 10., 10.]);
    assert!(inside(&s, 5., 5.));
    let s = shape(
        r#"<WidenedGeometry Thickness="2"><LineGeometry EndPoint="10,0"/><WidenedGeometry.Transform><ScaleTransform ScaleX="2" ScaleY="3"/></WidenedGeometry.Transform></WidenedGeometry>"#,
    );
    near(b(&s), [0., -3., 20., 6.]);
    let s = shape(
        r#"<WidenedGeometry Thickness="2"><LineGeometry EndPoint="10,0"><LineGeometry.Transform><ScaleTransform ScaleX="2" ScaleY="3"/></LineGeometry.Transform></LineGeometry></WidenedGeometry>"#,
    );
    near(b(&s), [0., -1., 20., 2.]);
    let s = shape(
        r#"<PathGeometry><PathFigure><LineSegment Point="10,0"/><LineSegment Point="10,10" IsStroked="False"/></PathFigure></PathGeometry>"#,
    );
    near(b(&s), [0., 0., 10., 0.]);
    assert!(inside(&s, 8., 2.));
}
#[test]
fn xaml_groups_override_rules_booleans_resolve_inputs() {
    let rings = r#"<PathGeometry FillRule="EvenOdd" Figures="M0,0 H10 V10 H0 Z M2,2 H8 V8 H2 Z"/>"#;
    let s = shape(&format!(
        r#"<GeometryGroup FillRule="Nonzero">{rings}</GeometryGroup>"#
    ));
    assert!(inside(&s, 5., 5.));
    let s = shape(&format!(
        r#"<CombinedGeometry GeometryCombineMode="Intersect"><CombinedGeometry.Geometry1>{rings}</CombinedGeometry.Geometry1><CombinedGeometry.Geometry2><RectangleGeometry Rect="3,3,4,4"/></CombinedGeometry.Geometry2></CombinedGeometry>"#
    ));
    assert!(!inside(&s, 5., 5.));
    let s = shape(
        r#"<CombinedGeometry GeometryCombineMode="Exclude"><CombinedGeometry.Geometry1><RectangleGeometry Rect="1000000,0,10,10"/></CombinedGeometry.Geometry1><CombinedGeometry.Geometry2><RectangleGeometry Rect="1000005,0,10,10"/></CombinedGeometry.Geometry2></CombinedGeometry>"#,
    );
    near(b(&s), [1000000., 0., 5., 10.]);
}
#[test]
fn xaml_caps_and_strict_input() {
    for (cap, expected) in [
        ("Flat", [0., -1., 10., 2.]),
        ("Square", [-1., -1., 12., 2.]),
        ("Round", [-1., -1., 12., 2.]),
        ("Triangle", [-1., -1., 12., 2.]),
    ] {
        let s = shape(&format!(
            r#"<WidenedGeometry Thickness="2"><LineGeometry EndPoint="10,0"/><WidenedGeometry.StrokeStyle><StrokeStyle StartLineCap="{cap}" EndLineCap="{cap}"/></WidenedGeometry.StrokeStyle></WidenedGeometry>"#
        ));
        near(b(&s), expected);
    }
    for bad in [
        r#"<Button/>"#,
        r#"<RectangleGeometry xmlns="wrong"/>"#,
        r#"<OutlinedGeometry Thickness="1"/>"#,
        r#"<PathGeometry><PathFigure><PolyCurveSegment Points="10,10 20,0"/></PathFigure></PathGeometry>"#,
        r#"<RectangleGeometry Bogus="0"/>"#,
        r#"<RectangleGeometry><EllipseGeometry.Transform><ScaleTransform/></EllipseGeometry.Transform></RectangleGeometry>"#,
    ] {
        assert!(
            parse(&format!(
                r#"<s:SimpleGeometryShape {NS}>{bad}</s:SimpleGeometryShape>"#
            ))
            .is_err(),
            "{bad}"
        );
    }
    assert!(parse(r#"<SimpleGeometryShape xmlns="clr-namespace:PaintDotNet.UI.Media;assembly=PaintDotNet.Framework" Geometry="M0,0 L1,1"/>"#).is_err());
    assert!(parse(&format!(r#"<s:SimpleGeometryShape {NS}/>"#)).is_err());
    assert!(
        parse(&format!(
            r#"<s:SimpleGeometryShape {NS} DisplayName="{}" Geometry="M0,0"/>"#,
            "n".repeat(81)
        ))
        .is_err()
    );
    assert!(parse(&"x".repeat(512 * 1024 + 1)).is_err());
    assert!(parse("<!DOCTYPE x><x/>").is_err());
    for p in [
        "M0,0 junk",
        "M0,0 L",
        "M0,0 A1,1 0 2 0 2,2",
        "MNaN,0",
        "L1,1",
    ] {
        assert!(parse_path_markup(p).is_err(), "{p}");
    }
}

#[test]
fn xaml_catalog_render_flags_and_transaction() {
    use pdn_core::{
        testing::{Ev, drive_tool, fresh_doc, layer_bytes, with_tool},
        tool::{MouseButton::Left, ToolType, shapes::ShapesTool},
    };
    let mut app = fresh_doc(80, 80, None);
    let custom = parse(&format!(
        r#"<s:SimpleGeometryShape {NS} DisplayName="Custom triangle" Geometry="M0,0 L20,0 0,10Z"/>"#
    ))
    .unwrap();
    app.shape_catalog.register("tri".into(), custom);
    let values = app.shape_catalog.values();
    assert_eq!(values[28], "heart");
    assert_eq!(values[29], "custom.tri");
    app.update_tool_setting(ToolType::Shapes, "shape", "heart".into());
    drive_tool(&mut app, ToolType::Shapes, &[Ev::Key("a")]);
    assert_eq!(
        app.tool_settings.get(ToolType::Shapes, "shape"),
        "custom.tri"
    );
    app.update_tool_setting(ToolType::Shapes, "drawType", "fill".into());
    let before = layer_bytes(&app, None);
    drive_tool(
        &mut app,
        ToolType::Shapes,
        &[
            Ev::Down(10., 10., Left),
            Ev::Move(50., 50.),
            Ev::Up(50., 50., Left),
        ],
    );
    with_tool::<ShapesTool>(&mut app, |tool, _| {
        assert!(tool.rendering_error().is_none());
    });
    drive_tool(&mut app, ToolType::Shapes, &[Ev::Key("Enter")]);
    let after = layer_bytes(&app, None);
    assert_ne!(before, after);
    assert!(
        app.active_ws()
            .unwrap()
            .history
            .undo
            .iter()
            .any(|m| m.name.contains("Custom triangle"))
    );
    assert!(app.undo());
    assert!(
        layer_bytes(&app, None) == after,
        "Finish undo reopens the preview"
    );
    assert!(app.undo());
    assert!(
        layer_bytes(&app, None) == before,
        "Draw undo cancels the preview"
    );
    assert!(app.redo());
    assert!(app.redo());
    assert!(layer_bytes(&app, None) == after);
    let hollow = shape(
        r#"<PathGeometry><PathFigure IsFilled="False" IsClosed="True"><LineSegment Point="10,0"/><LineSegment Point="10,10"/><LineSegment Point="0,10"/></PathFigure></PathGeometry>"#,
    );
    let mut c = ShapeCatalog::default();
    c.register("hollow".into(), hollow);
    let d = c
        .render_data(
            "hollow",
            [10., 10., 20., 20.],
            2.,
            DrawType::Both,
            DashStyle::Solid,
        )
        .unwrap()
        .unwrap();
    let (p, r) = d.interior.unwrap();
    assert!(
        shapes::coverage_mask(&p, r, None, false, IntRect::new(0, 0, 40, 40))
            .iter()
            .all(|v| *v == 0)
    );
    let (p, r) = d.outline.unwrap();
    assert!(!p.contains(20., 20., r));
    assert!(p.contains(10., 20., r));
}
#[test]
fn xaml_property_forms_transform_order_and_resource_limits() {
    let s = shape(
        r#"<RectangleGeometry><RectangleGeometry.Rect><p:RectDouble><p:RectDouble.Location><p:Point2Double>1,2</p:Point2Double></p:RectDouble.Location><p:RectDouble.Size><p:SizeDouble Width="3" Height="4"/></p:RectDouble.Size></p:RectDouble></RectangleGeometry.Rect><RectangleGeometry.Transform><TransformGroup><TransformGroup.Children><TransformCollection><ScaleTransform ScaleX="2" ScaleY="3"/><TranslateTransform X="5" Y="7"/></TransformCollection></TransformGroup.Children></TransformGroup></RectangleGeometry.Transform></RectangleGeometry>"#,
    );
    near(b(&s), [7., 13., 6., 12.]);
    let s = shape(
        r#"<FlattenedGeometry AllowCurves="True"><PathGeometry Figures="M0,0 Q5,10 10,0"/></FlattenedGeometry>"#,
    );
    near(b(&s), [0., 0., 10., 5.]);
    let enormous = format!(
        r#"<s:SimpleGeometryShape {NS}><FlattenedGeometry FlatteningTolerance="0.000001"><EllipseGeometry RadiusX="1000000000" RadiusY="1000000000"/></FlattenedGeometry></s:SimpleGeometryShape>"#
    );
    assert!(parse(&enormous).is_err());
    for bad in [
        r#"<GeometryGroup Children="M0,0"/>"#,
        r#"<RectangleGeometry Rect="0,0,2,2"><RectangleGeometry.Rect>1,1,2,2</RectangleGeometry.Rect></RectangleGeometry>"#,
        r#"<PathGeometry xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml" x:Class="Bad"/>"#,
        r#"<WidenedGeometry><WidenedGeometry.StrokeStyle><StrokeStyle><StrokeStyle.DashStyle><DashStyle Dashes="1,2"/></StrokeStyle.DashStyle></StrokeStyle></WidenedGeometry.StrokeStyle></WidenedGeometry>"#,
    ] {
        assert!(
            parse(&format!(
                r#"<s:SimpleGeometryShape {NS}>{bad}</s:SimpleGeometryShape>"#
            ))
            .is_err()
        );
    }
}
#[test]
fn xaml_widen_round_caps_smooth_closure_and_explicit_dashes() {
    let s = shape(
        r#"<WidenedGeometry Thickness="2"><LineGeometry EndPoint="10,0"/><WidenedGeometry.StrokeStyle><StrokeStyle StartLineCap="Round" EndLineCap="Round"/></WidenedGeometry.StrokeStyle></WidenedGeometry>"#,
    );
    for x in [-0.5, 0.5, 9.5, 10.5] {
        assert!(inside(&s, x, 0.), "cap/stroke union at {x}");
    }
    let s = shape(
        r#"<WidenedGeometry Thickness="2"><PathGeometry><PathFigure IsClosed="True"><LineSegment Point="10,0" IsSmoothJoin="True"/><LineSegment Point="10,10"/><LineSegment Point="0,10"/></PathFigure></PathGeometry></WidenedGeometry>"#,
    );
    assert!(
        inside(&s, -0.5, -0.5),
        "round join at the closed first vertex"
    );
    assert!(
        !inside(&s, -0.9, -0.9),
        "forced round join removes miter corner"
    );
    assert!(inside(&s, 10.8, 10.8), "other joins remain miter");
    let s = shape(
        r#"<WidenedGeometry Thickness="2"><LineGeometry EndPoint="20,0"/><WidenedGeometry.StrokeStyle><StrokeStyle DashCap="Flat"><StrokeStyle.DashStyle><DashStyle><DashStyle.Dashes><o:DoubleCollection><sys:Double>2</sys:Double><sys:Double>2</sys:Double></o:DoubleCollection></DashStyle.Dashes></DashStyle></StrokeStyle.DashStyle></StrokeStyle></WidenedGeometry.StrokeStyle></WidenedGeometry>"#,
    );
    assert!(inside(&s, 2., 0.));
    assert!(!inside(&s, 6., 0.));
    assert!(inside(&s, 10., 0.));
    let dots = shape(
        r#"<WidenedGeometry Thickness="2"><LineGeometry EndPoint="20,0"/><WidenedGeometry.StrokeStyle><StrokeStyle DashCap="Square"><StrokeStyle.DashStyle><DashStyle><DashStyle.Dashes><sys:Double>0</sys:Double><sys:Double>2</sys:Double></DashStyle.Dashes></DashStyle></StrokeStyle.DashStyle></StrokeStyle></WidenedGeometry.StrokeStyle></WidenedGeometry>"#,
    );
    assert!(
        inside(&dots, 4., 0.),
        "zero-length painted intervals still have square caps"
    );
}
#[test]
fn xaml_typed_figure_text_and_markup_separators() {
    let s = shape(
        r#"<PathGeometry><PathGeometry.Figures><PathFigureCollection>M0,0 L10,0 0,10Z</PathFigureCollection></PathGeometry.Figures></PathGeometry>"#,
    );
    near(b(&s), [0., 0., 10., 10.]);
    assert!(parse_path_markup("M,0,0 L1,1").is_err());
    assert!(parse_path_markup("M0,0 L,1,1").is_err());
}

#[test]
fn xaml_finite_operation_limits_are_errors() {
    let huge = format!(
        r#"<s:SimpleGeometryShape {NS}><WidenedGeometry><LineGeometry StartPoint="1e80,0" EndPoint="2e80,0"/></WidenedGeometry></s:SimpleGeometryShape>"#
    );
    assert!(
        parse(&huge).is_err(),
        "the f32 stroker must not silently discard overflowing coordinates"
    );
    let tiny = format!(
        r#"<s:SimpleGeometryShape {NS}><CombinedGeometry><CombinedGeometry.Geometry1><RectangleGeometry Rect="0,0,1e-320,1e-320"/></CombinedGeometry.Geometry1></CombinedGeometry></s:SimpleGeometryShape>"#
    );
    assert!(
        parse(&tiny).is_err(),
        "normalization overflow must fail before the boolean engine"
    );
}

#[test]
fn xaml_names_collections_closure_and_rule_override() {
    let s = shape(
        r#"<GeometryGroup FillRule="EvenOdd"><GeometryCollection xmlns="clr-namespace:PaintDotNet.UI.Media;assembly=PaintDotNet.Framework"><PathGeometry FillRule="Nonzero" Figures="M0,0H10V10H0Z M2,2H8V8H2Z"/></GeometryCollection></GeometryGroup>"#,
    );
    assert!(!inside(&s, 5., 5.));
    let s = shape(
        r#"<PathGeometry xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml" x:Name="path"><PathGeometry.Figures><PathFigureCollection><PathFigure IsClosed="True"><PathFigure.Segments><PathSegmentCollection><LineSegment Point="10,0"/><LineSegment Point="10,10" IsStroked="False"/></PathSegmentCollection></PathFigure.Segments></PathFigure></PathFigureCollection></PathGeometry.Figures></PathGeometry>"#,
    );
    near(b(&s), [0., 0., 10., 0.]);
    assert!(inside(&s, 8., 2.));
    let s = shape(
        r#"<WidenedGeometry Thickness="2"><PathGeometry><PathFigure IsClosed="True"><LineSegment Point="10,0"/><LineSegment Point="10,10" IsStroked="False"/></PathFigure></PathGeometry></WidenedGeometry>"#,
    );
    near(b(&s), [0., -1., 10., 2.]);
    let s = shape(
        r#"<PathGeometry><PathFigure><PolyBezierSegment><PolyBezierSegment.Points><PointCollection><p:Point2Double>0,10</p:Point2Double><p:Point2Double>10,10</p:Point2Double><p:Point2Double>10,0</p:Point2Double></PointCollection></PolyBezierSegment.Points></PolyBezierSegment><PolyQuadraticBezierSegment><PolyQuadraticBezierSegment.Points><p:Point2Double>5,-10</p:Point2Double><p:Point2Double>0,0</p:Point2Double></PolyQuadraticBezierSegment.Points></PolyQuadraticBezierSegment></PathFigure></PathGeometry>"#,
    );
    near(b(&s), [0., -5., 10., 12.5]);
}

#[test]
fn xaml_catalog_ids_do_not_shadow_builtins_and_history_retains_geometry() {
    use pdn_core::{
        testing::{Ev, drive_tool, fresh_doc, with_tool},
        tool::{MouseButton::Left, ToolType, shapes::ShapesTool},
    };
    let mut app = fresh_doc(80, 80, None);
    app.shape_catalog.register(
        "rectangle".into(),
        shape(r#"<EllipseGeometry Center="5,5" RadiusX="5" RadiusY="5"/>"#),
    );
    drive_tool(
        &mut app,
        ToolType::Shapes,
        &[
            Ev::Down(10., 10., Left),
            Ev::Move(40., 40.),
            Ev::Up(40., 40., Left),
        ],
    );
    with_tool::<ShapesTool>(&mut app, |t, _| {
        assert!(
            t.context().unwrap().custom_shape.is_none(),
            "builtin ids must not address the custom catalogue"
        )
    });
    app.update_tool_setting(ToolType::Shapes, "shape", "custom.rectangle".into());
    drive_tool(&mut app, ToolType::Shapes, &[Ev::Wait(600.)]);
    let mut original = None;
    with_tool::<ShapesTool>(&mut app, |t, _| {
        original = t.context().unwrap().custom_shape.clone()
    });
    app.shape_catalog.register(
        "rectangle".into(),
        shape(r#"<RectangleGeometry Rect="0,0,20,10"/>"#),
    );
    app.update_tool_setting(ToolType::Shapes, "width", 7.into());
    with_tool::<ShapesTool>(&mut app, |t, _| {
        assert_eq!(
            t.context().unwrap().custom_shape,
            original,
            "an unrelated setting preserves the retained geometry"
        )
    });
}
#[test]
fn xaml_boolean_work_budget_is_explicit() {
    let mut content = String::from("<OutlinedGeometry><GeometryGroup>");
    for _ in 0..400 {
        content.push_str(r#"<RectangleGeometry Rect="0,0,10,10"/>"#);
    }
    content.push_str("</GeometryGroup></OutlinedGeometry>");
    let result = parse(&format!(
        r#"<s:SimpleGeometryShape {NS}>{content}</s:SimpleGeometryShape>"#
    ));
    assert!(
        result.is_err(),
        "dense overlapping edges must fail before unbounded intersection work"
    );
}

#[test]
fn xaml_infinite_root_policy_and_nested_rejections() {
    let s = shape(
        r#"<InfiniteGeometry><InfiniteGeometry.Transform><TranslateTransform X="4"/></InfiniteGeometry.Transform></InfiniteGeometry>"#,
    );
    let mut catalog = ShapeCatalog::default();
    catalog.register("infinite".into(), s);
    assert!(
        catalog
            .create_path("infinite", [0., 0., 20., 20.])
            .unwrap()
            .is_empty()
    );
    for family in [
        "GeometryGroup",
        "TransformedGeometry",
        "FlattenedGeometry",
        "OutlinedGeometry",
        "WidenedGeometry",
    ] {
        let text = format!(
            r#"<s:SimpleGeometryShape {NS}><{family}><InfiniteGeometry/></{family}></s:SimpleGeometryShape>"#
        );
        let e = parse(&text).unwrap_err().to_string();
        assert!(e.contains("InfiniteGeometry"), "{family}: {e}");
    }
    for operand in ["Geometry1", "Geometry2"] {
        let text = format!(
            r#"<s:SimpleGeometryShape {NS}><CombinedGeometry><CombinedGeometry.{operand}><InfiniteGeometry/></CombinedGeometry.{operand}></CombinedGeometry></s:SimpleGeometryShape>"#
        );
        assert!(
            parse(&text)
                .unwrap_err()
                .to_string()
                .contains("InfiniteGeometry")
        );
    }
}

#[test]
fn xaml_smooth_join_does_not_restart_dash_phase_or_paint_gaps() {
    let style = r#"<WidenedGeometry.StrokeStyle><StrokeStyle DashCap="Flat"><StrokeStyle.DashStyle><DashStyle><DashStyle.Dashes><sys:Double>2</sys:Double><sys:Double>2</sys:Double></DashStyle.Dashes></DashStyle></StrokeStyle.DashStyle></StrokeStyle></WidenedGeometry.StrokeStyle>"#;
    let s = shape(&format!(
        r#"<WidenedGeometry Thickness="2"><PathGeometry><PathFigure><LineSegment Point="6,0"/><LineSegment Point="6,10" IsSmoothJoin="True"/></PathFigure></PathGeometry>{style}</WidenedGeometry>"#
    ));
    assert!(
        !inside(&s, 6., 0.),
        "a forced join in the unpainted dash interval must stay empty"
    );
    assert!(
        !inside(&s, 6., 1.),
        "dash phase continues across the smooth join"
    );
    assert!(inside(&s, 6., 3.));
}

#[test]
fn xaml_matrix_skew_and_smooth_markup() {
    let s = shape(
        r#"<RectangleGeometry Rect="0,0,4,2"><RectangleGeometry.Transform><MatrixTransform Matrix="2,1,-1,3,5,7"/></RectangleGeometry.Transform></RectangleGeometry>"#,
    );
    near(b(&s), [3., 7., 10., 10.]);
    let s = shape(
        r#"<RectangleGeometry Rect="0,0,4,2"><RectangleGeometry.Transform><SkewTransform AngleX="45" CenterY="1"/></RectangleGeometry.Transform></RectangleGeometry>"#,
    );
    near(b(&s), [-1., 0., 6., 2.]);
    let a = parse_path_markup("M0 0 C1 3 2 3 3 0 s2 -3 3 0 Q7 3 8 0 t2 0").unwrap();
    let explicit =
        parse_path_markup("M0 0 C1 3 2 3 3 0 C4 -3 5 -3 6 0 Q7 3 8 0 Q9 -3 10 0").unwrap();
    assert_eq!(a, explicit);
    let s = shape(
        r#"<TransformedGeometry Transform="Identity"><LineGeometry EndPoint="3,4"><LineGeometry.Transform><IdentityTransform/></LineGeometry.Transform></LineGeometry></TransformedGeometry>"#,
    );
    near(b(&s), [0., 0., 3., 4.]);
}

#[test]
fn xaml_custom_aspect_and_reverse_catalog_cycle() {
    use pdn_core::{
        testing::{Ev, drive_tool, fresh_doc, with_tool},
        tool::{MouseButton::Left, ToolType, shapes::ShapesTool},
    };
    let mut app = fresh_doc(80, 80, None);
    app.shape_catalog.register(
        "wide".into(),
        shape(r#"<RectangleGeometry Rect="0,0,20,10"/>"#),
    );
    app.update_tool_setting(ToolType::Shapes, "shape", "rectangle".into());
    drive_tool(&mut app, ToolType::Shapes, &[Ev::Key("Shift+a")]);
    assert_eq!(
        app.tool_settings.get(ToolType::Shapes, "shape"),
        "custom.wide"
    );
    drive_tool(
        &mut app,
        ToolType::Shapes,
        &[
            Ev::DownMods(10., 10., Left, "Shift"),
            Ev::Move(50., 50.),
            Ev::Up(50., 50., Left),
        ],
    );
    with_tool::<ShapesTool>(&mut app, |tool, _| {
        let s = tool.context().unwrap();
        let width = (s.end[0] - s.start[0]).abs();
        let height = (s.end[1] - s.start[1]).abs();
        assert!((width / height - 2.).abs() < 1e-8);
    });
}

#[test]
fn xaml_rect_value_preserves_overlapping_setter_order() {
    for (rect, expected) in [
        (
            r#"<p:RectDouble Location="1,2" X="9" Y="10" Size="3,4" Width="8" Height="7"/>"#,
            [9., 10., 8., 7.],
        ),
        (
            r#"<p:RectDouble X="9" Y="10" Location="1,2" Width="8" Height="7" Size="3,4"/>"#,
            [1., 2., 3., 4.],
        ),
        (
            r#"<p:RectDouble><p:RectDouble.Location><p:Point2Double X="1" Y="2"/></p:RectDouble.Location><p:RectDouble.X><sys:Double>9</sys:Double></p:RectDouble.X><p:RectDouble.Y>10</p:RectDouble.Y><p:RectDouble.Size><p:SizeDouble>3,4</p:SizeDouble></p:RectDouble.Size><p:RectDouble.Width>8</p:RectDouble.Width><p:RectDouble.Height><sys:Double>7</sys:Double></p:RectDouble.Height></p:RectDouble>"#,
            [9., 10., 8., 7.],
        ),
        (
            r#"<p:RectDouble><p:RectDouble.X>9</p:RectDouble.X><p:RectDouble.Y><sys:Double>10</sys:Double></p:RectDouble.Y><p:RectDouble.Location><p:Point2Double>1,2</p:Point2Double></p:RectDouble.Location><p:RectDouble.Width><sys:Double>8</sys:Double></p:RectDouble.Width><p:RectDouble.Height>7</p:RectDouble.Height><p:RectDouble.Size><p:SizeDouble Width="3" Height="4"/></p:RectDouble.Size></p:RectDouble>"#,
            [1., 2., 3., 4.],
        ),
        (
            r#"<p:RectDouble Location="1,2" Size="3,4"><p:RectDouble.X>9</p:RectDouble.X><p:RectDouble.Width>8</p:RectDouble.Width></p:RectDouble>"#,
            [9., 2., 8., 4.],
        ),
        (
            r#"<p:RectDouble X="9" Y="10" Width="8" Height="7"><p:RectDouble.Location>1,2</p:RectDouble.Location><p:RectDouble.Size>3,4</p:RectDouble.Size></p:RectDouble>"#,
            [1., 2., 3., 4.],
        ),
    ] {
        let s = shape(&format!(
            "<RectangleGeometry><RectangleGeometry.Rect>{rect}</RectangleGeometry.Rect></RectangleGeometry>"
        ));
        near(b(&s), expected);
    }
    for property in ["X", "Y", "Width", "Height", "Location", "Size"] {
        let value = if matches!(property, "Location" | "Size") {
            "1,2"
        } else {
            "1"
        };
        let source = format!(
            r#"<s:SimpleGeometryShape {NS}><RectangleGeometry><RectangleGeometry.Rect><p:RectDouble {property}="{value}"><p:RectDouble.{property}>{value}</p:RectDouble.{property}></p:RectDouble></RectangleGeometry.Rect></RectangleGeometry></s:SimpleGeometryShape>"#
        );
        assert!(
            matches!(parse(&source),Err(XamlError::BadAttribute(message)) if message.contains("duplicate")),
            "{property}"
        );
    }
}

#[test]
fn xaml_collections_reject_synthetic_items_members() {
    let cases = [
        (
            "GeometryCollection",
            r#"<RectangleGeometry Rect="0,0,10,10"/>"#,
            "<GeometryGroup><GeometryGroup.Children>",
            "</GeometryGroup.Children></GeometryGroup>",
        ),
        (
            "TransformCollection",
            r#"<TranslateTransform X="2"/>"#,
            r#"<RectangleGeometry Rect="0,0,10,10"><RectangleGeometry.Transform><TransformGroup><TransformGroup.Children>"#,
            "</TransformGroup.Children></TransformGroup></RectangleGeometry.Transform></RectangleGeometry>",
        ),
        (
            "PathFigureCollection",
            r#"<PathFigure><LineSegment Point="10,10"/></PathFigure>"#,
            "<PathGeometry><PathGeometry.Figures>",
            "</PathGeometry.Figures></PathGeometry>",
        ),
        (
            "PathSegmentCollection",
            r#"<LineSegment Point="10,10"/>"#,
            "<PathGeometry><PathFigure><PathFigure.Segments>",
            "</PathFigure.Segments></PathFigure></PathGeometry>",
        ),
        (
            "PointCollection",
            r#"<p:Point2Double X="10" Y="10"/>"#,
            "<PathGeometry><PathFigure><PolyLineSegment><PolyLineSegment.Points>",
            "</PolyLineSegment.Points></PolyLineSegment></PathFigure></PathGeometry>",
        ),
        (
            "o:DoubleCollection",
            "<sys:Double>2</sys:Double><sys:Double>2</sys:Double>",
            r#"<WidenedGeometry><LineGeometry EndPoint="10,0"/><WidenedGeometry.StrokeStyle><StrokeStyle><StrokeStyle.DashStyle><DashStyle><DashStyle.Dashes>"#,
            "</DashStyle.Dashes></DashStyle></StrokeStyle.DashStyle></StrokeStyle></WidenedGeometry.StrokeStyle></WidenedGeometry>",
        ),
    ];
    let mut accepted = vec![];
    for (collection, item, before, after) in cases {
        let source = |body: String| {
            format!(r#"<s:SimpleGeometryShape {NS}>{before}{body}{after}</s:SimpleGeometryShape>"#)
        };
        // Direct content, owner collection properties and inherited Name remain valid.
        for body in [
            format!(r#"<{collection} Name="points">{item}</{collection}>"#),
            format!(
                "<{collection}><{collection}.Name>points</{collection}.Name>{item}</{collection}>"
            ),
        ] {
            assert!(parse(&source(body)).is_ok(), "{collection} valid content");
        }
        for body in [
            format!("<{collection}><{collection}.Items>{item}</{collection}.Items></{collection}>"),
            format!(r#"<{collection} Items="0">{item}</{collection}>"#),
        ] {
            let name = collection.rsplit(':').next().unwrap();
            if !matches!(parse(&source(body)), Err(XamlError::BadAttribute(message)) if message==format!("{name}.Items"))
            {
                accepted.push(collection);
            }
        }
    }
    assert!(
        accepted.is_empty(),
        "synthetic members were not rejected as unsupported: {accepted:?}"
    );
    assert!(parse(&format!(r#"<s:SimpleGeometryShape {NS}><GeometryGroup><GeometryCollection><LineSegment/></GeometryCollection></GeometryGroup></s:SimpleGeometryShape>"#)).is_err());
}
