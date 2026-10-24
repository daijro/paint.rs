use super::{Geometry, MAX_SEGMENTS, Result, XamlError, invalid};
use super::{
    geometry::{self, IDENTITY, StrokeStyle},
    markup::{numbers, parse_path_markup},
};
use crate::{
    canvas::{
        FillRule, Path2D,
        path::{AnalyticArc, PathFlags},
    },
    selection::CombineMode,
};
use std::{
    collections::BTreeMap,
    f64::consts::{PI, TAU},
};
// The XML namespaces Paint.NET shape files (.xaml) declare: file-format data, matched as strings.
const MEDIA: &str = "clr-namespace:PaintDotNet.UI.Media;assembly=PaintDotNet.UI";
const FORWARD: &str = "clr-namespace:PaintDotNet.UI.Media;assembly=PaintDotNet.Framework";
const SHAPES: &str = "clr-namespace:PaintDotNet.Shapes;assembly=PaintDotNet.Framework";
const PRIMITIVES: &str = "clr-namespace:PaintDotNet.Rendering;assembly=PaintDotNet.Primitives";
const OBJECTS: &str = "clr-namespace:PaintDotNet.ObjectModel;assembly=PaintDotNet.ObjectModel";
const DIRECT: &str = "clr-namespace:PaintDotNet.Direct2D1;assembly=PaintDotNet.Windows";
const XAML: &str = "http://schemas.microsoft.com/winfx/2006/xaml";
#[derive(Clone, Debug)]
pub(super) enum Value {
    Text(String),
    Objects(Vec<Object>),
}
#[derive(Clone, Debug)]
pub(super) struct Object {
    pub kind: String,
    pub props: BTreeMap<String, Value>,
    member_order: Vec<String>,
    collection_items: Vec<Object>,
    text: Option<String>,
}
fn is_geometry(k: &str) -> bool {
    matches!(
        k,
        "CombinedGeometry"
            | "EllipseGeometry"
            | "EmptyGeometry"
            | "FlattenedGeometry"
            | "GeometryGroup"
            | "InfiniteGeometry"
            | "LineGeometry"
            | "OutlinedGeometry"
            | "PathGeometry"
            | "RectangleGeometry"
            | "TransformedGeometry"
            | "WidenedGeometry"
    )
}
fn is_transform(k: &str) -> bool {
    matches!(
        k,
        "IdentityTransform"
            | "MatrixTransform"
            | "RotateTransform"
            | "ScaleTransform"
            | "SkewTransform"
            | "TransformGroup"
            | "TranslateTransform"
    )
}
fn is_segment(k: &str) -> bool {
    matches!(
        k,
        "ArcSegment"
            | "BezierSegment"
            | "LineSegment"
            | "PolyBezierSegment"
            | "PolyCurveSegment"
            | "PolyLineSegment"
            | "PolyQuadraticBezierSegment"
            | "QuadraticBezierSegment"
    )
}
fn collection(k: &str) -> Option<&'static str> {
    match k {
        "GeometryCollection" => Some("Geometry"),
        "TransformCollection" => Some("Transform"),
        "PathFigureCollection" => Some("PathFigure"),
        "PathSegmentCollection" => Some("PathSegment"),
        "PointCollection" => Some("Point2Double"),
        "DoubleCollection" => Some("Double"),
        _ => None,
    }
}
fn value_type(k: &str) -> bool {
    matches!(
        k,
        "Double"
            | "Boolean"
            | "Point2Double"
            | "SizeDouble"
            | "RectDouble"
            | "Matrix3x2Double"
            | "FillRule"
            | "PenLineCap"
            | "PenLineJoin"
            | "GeometryCombineMode"
            | "SweepDirection"
    )
}
fn namespace(k: &str, ns: Option<&str>, base: bool) -> bool {
    let ns = ns.unwrap_or("");
    if k == "SimpleGeometryShape" || base && k == "DeclarativeShape" {
        return ns == SHAPES;
    }
    if matches!(
        k,
        "Point2Double" | "SizeDouble" | "RectDouble" | "Matrix3x2Double" | "GeometryCombineMode"
    ) {
        return ns == PRIMITIVES;
    }
    if k == "DoubleCollection" || base && k == "DependencyResource" {
        return ns == OBJECTS;
    }
    if k == "SweepDirection" {
        return ns == DIRECT;
    }
    if matches!(k, "Double" | "Boolean") {
        return matches!(
            ns,
            "clr-namespace:System;assembly=mscorlib"
                | "clr-namespace:System;assembly=System.Private.CoreLib"
        );
    }
    (is_geometry(k)
        || is_transform(k)
        || is_segment(k)
        || collection(k).is_some()
        || matches!(
            k,
            "PathFigure" | "StrokeStyle" | "DashStyle" | "FillRule" | "PenLineCap" | "PenLineJoin"
        )
        || base && matches!(k, "Geometry" | "Transform" | "PathSegment"))
        && (ns == MEDIA || ns == FORWARD)
}
fn content(k: &str) -> Option<&'static str> {
    match k {
        "SimpleGeometryShape"
        | "FlattenedGeometry"
        | "OutlinedGeometry"
        | "WidenedGeometry"
        | "TransformedGeometry" => Some("Geometry"),
        "GeometryGroup" | "TransformGroup" => Some("Children"),
        "PathGeometry" => Some("Figures"),
        "PathFigure" => Some("Segments"),
        _ => None,
    }
}
fn property(k: &str, name: &str) -> Option<&'static str> {
    if name == "Name" && !value_type(k) && k != "SimpleGeometryShape" {
        return Some("String");
    }
    if name == "Transform" && (is_geometry(k) || k == "Geometry") {
        return Some("Transform");
    }
    if matches!(name, "IsStroked" | "IsSmoothJoin") && (is_segment(k) || k == "PathSegment") {
        return Some("Boolean");
    }
    match (k, name) {
        ("SimpleGeometryShape" | "DeclarativeShape", "DisplayName") => Some("String"),
        (
            "SimpleGeometryShape"
            | "FlattenedGeometry"
            | "OutlinedGeometry"
            | "WidenedGeometry"
            | "TransformedGeometry",
            "Geometry",
        ) => Some("Geometry"),
        ("FlattenedGeometry", "AllowCurves") => Some("Boolean"),
        (
            "FlattenedGeometry" | "OutlinedGeometry" | "WidenedGeometry" | "CombinedGeometry",
            "FlatteningTolerance",
        ) => Some("Double"),
        ("WidenedGeometry", "Thickness") => Some("Double"),
        ("WidenedGeometry", "StrokeStyle") => Some("StrokeStyle"),
        ("EllipseGeometry", "Center") => Some("Point2Double"),
        ("EllipseGeometry" | "RectangleGeometry", "RadiusX" | "RadiusY") => Some("Double"),
        ("RectangleGeometry", "Rect") => Some("RectDouble"),
        ("LineGeometry", "StartPoint" | "EndPoint") => Some("Point2Double"),
        ("PathGeometry" | "GeometryGroup", "FillRule") => Some("FillRule"),
        ("PathGeometry", "Figures") => Some("PathFigureCollection"),
        ("GeometryGroup", "Children") => Some("GeometryCollection"),
        ("CombinedGeometry", "Geometry1" | "Geometry2") => Some("Geometry"),
        ("CombinedGeometry", "GeometryCombineMode") => Some("GeometryCombineMode"),
        ("PathFigure", "StartPoint") => Some("Point2Double"),
        ("PathFigure", "IsClosed" | "IsFilled") => Some("Boolean"),
        ("PathFigure", "Segments") => Some("PathSegmentCollection"),
        ("ArcSegment" | "LineSegment", "Point") => Some("Point2Double"),
        ("ArcSegment", "Size") => Some("SizeDouble"),
        ("ArcSegment", "RotationAngle") => Some("Double"),
        ("ArcSegment", "IsLargeArc") => Some("Boolean"),
        ("ArcSegment", "SweepDirection") => Some("SweepDirection"),
        ("BezierSegment", "Point1" | "Point2" | "Point3")
        | ("QuadraticBezierSegment", "Point1" | "Point2") => Some("Point2Double"),
        (
            "PolyLineSegment"
            | "PolyBezierSegment"
            | "PolyQuadraticBezierSegment"
            | "PolyCurveSegment",
            "Points",
        ) => Some("PointCollection"),
        ("MatrixTransform", "Matrix") => Some("Matrix3x2Double"),
        ("RotateTransform", "Angle" | "CenterX" | "CenterY")
        | ("ScaleTransform", "ScaleX" | "ScaleY" | "CenterX" | "CenterY")
        | ("SkewTransform", "AngleX" | "AngleY" | "CenterX" | "CenterY")
        | ("TranslateTransform", "X" | "Y") => Some("Double"),
        ("TransformGroup", "Children") => Some("TransformCollection"),
        ("StrokeStyle", "StartLineCap" | "EndLineCap" | "DashCap") => Some("PenLineCap"),
        ("StrokeStyle", "LineJoin") => Some("PenLineJoin"),
        ("StrokeStyle", "MiterLimit") => Some("Double"),
        ("StrokeStyle", "DashStyle") => Some("DashStyle"),
        ("StrokeStyle", "TransformType") => Some("StrokeTransformType"),
        ("DashStyle", "Dashes") => Some("DoubleCollection"),
        ("DashStyle", "Offset") => Some("Double"),
        ("Point2Double", "X" | "Y")
        | ("SizeDouble", "Width" | "Height")
        | ("RectDouble", "X" | "Y" | "Width" | "Height")
        | ("Matrix3x2Double", "M11" | "M12" | "M21" | "M22" | "OffsetX" | "OffsetY") => {
            Some("Double")
        }
        ("RectDouble", "Location") => Some("Point2Double"),
        ("RectDouble", "Size") => Some("SizeDouble"),
        _ => None,
    }
}
fn owner_applies(owner: &str, k: &str) -> bool {
    owner == k
        || owner == "Geometry" && is_geometry(k)
        || owner == "Transform" && is_transform(k)
        || owner == "PathSegment" && is_segment(k)
        || owner == "DependencyResource" && !value_type(k) && k != "SimpleGeometryShape"
        || owner == "DeclarativeShape" && k == "SimpleGeometryShape"
}
fn text_value(s: &str) -> Result<String> {
    if let Some(s) = s.strip_prefix("{}") {
        Ok(s.into())
    } else if s.trim_start().starts_with('{') {
        Err(XamlError::NotAllowed("markup extension".into()))
    } else {
        Ok(s.into())
    }
}
fn insert(o: &mut Object, k: &str, v: Value) -> Result<()> {
    if property(&o.kind, k).is_none() {
        return Err(invalid(format!("{}.{}", o.kind, k)));
    }
    if o.props.insert(k.into(), v).is_some() {
        return Err(invalid(format!("duplicate {}.{}", o.kind, k)));
    }
    o.member_order.push(k.into());
    Ok(())
}
fn object(n: roxmltree::Node<'_, '_>, depth: usize) -> Result<Object> {
    if depth > 64 {
        return Err(invalid("XAML nesting limit"));
    }
    let k = n.tag_name().name();
    if !namespace(k, n.tag_name().namespace(), false) {
        return Err(XamlError::NotAllowed(format!(
            "{{{}}}{k}",
            n.tag_name().namespace().unwrap_or("")
        )));
    }
    let mut o = Object {
        kind: k.into(),
        props: BTreeMap::new(),
        member_order: Vec::new(),
        collection_items: Vec::new(),
        text: None,
    };
    for a in n.attributes() {
        let name = if a.namespace() == Some(XAML) && a.name() == "Name" {
            "Name"
        } else if a.namespace().is_none() {
            a.name()
        } else {
            return Err(XamlError::NotAllowed(a.name().into()));
        };
        insert(&mut o, name, Value::Text(text_value(a.value())?))?;
    }
    let mut children = Vec::new();
    let mut text = String::new();
    for c in n.children() {
        if c.is_text() {
            text.push_str(c.text().unwrap_or(""));
            continue;
        }
        if !c.is_element() {
            continue;
        }
        if let Some((owner, key)) = c.tag_name().name().split_once('.') {
            if !namespace(owner, c.tag_name().namespace(), true)
                || !owner_applies(owner, k)
                || property(owner, key).is_none()
                || c.attributes().len() != 0
            {
                return Err(invalid(c.tag_name().name()));
            }
            let mut nodes = Vec::new();
            let mut txt = String::new();
            for item in c.children() {
                if item.is_element() {
                    nodes.push(object(item, depth + 1)?)
                } else if item.is_text() {
                    txt.push_str(item.text().unwrap_or(""))
                }
            }
            if !nodes.is_empty() && !txt.trim().is_empty() {
                return Err(invalid("mixed property content"));
            }
            insert(
                &mut o,
                key,
                if nodes.is_empty() {
                    Value::Text(text_value(txt.trim())?)
                } else {
                    Value::Objects(nodes)
                },
            )?;
        } else {
            children.push(object(c, depth + 1)?)
        }
    }
    if !children.is_empty() && !text.trim().is_empty() {
        return Err(invalid("mixed object content"));
    }
    if !children.is_empty() {
        if let Some(item) = collection(k) {
            // Collection addition is content syntax, not a writable Items member (§9.4.2).
            for child in &children {
                if !compatible(&child.kind, item) {
                    return Err(invalid(format!("expected {item}, got {}", child.kind)));
                }
            }
            o.collection_items = children;
        } else {
            let key = content(k).ok_or_else(|| invalid(format!("{k} has no content property")))?;
            insert(&mut o, key, Value::Objects(children))?;
        }
    } else if !text.trim().is_empty() {
        if value_type(k) || k == "PathFigureCollection" {
            if !o.props.is_empty() {
                return Err(invalid("value text with properties"));
            }
            o.text = Some(text_value(text.trim())?);
        } else {
            let key = content(k).ok_or_else(|| invalid(format!("{k} has no text content")))?;
            insert(&mut o, key, Value::Text(text_value(text.trim())?))?;
        }
    }
    for (key, v) in &o.props {
        validate(v, property(k, key).unwrap())?;
    }
    if let Some(s) = &o.text {
        validate_text(s, k)?;
    }
    Ok(o)
}
pub(super) fn read(source: &str) -> Result<Object> {
    let doc = roxmltree::Document::parse_with_options(
        source,
        roxmltree::ParsingOptions {
            allow_dtd: false,
            nodes_limit: 32768,
        },
    )
    .map_err(|e| XamlError::NotXml(e.to_string()))?;
    let n = doc.root_element();
    if n.tag_name().name() != "SimpleGeometryShape" || n.tag_name().namespace() != Some(SHAPES) {
        return Err(XamlError::Root(n.tag_name().name().into()));
    }
    object(n, 0)
}
fn compatible(actual: &str, expected: &str) -> bool {
    actual == expected
        || expected == "Geometry" && is_geometry(actual)
        || expected == "Transform" && is_transform(actual)
        || expected == "PathSegment" && is_segment(actual)
}
fn validate(v: &Value, ty: &str) -> Result<()> {
    match v {
        Value::Text(s) => validate_text(s, ty),
        Value::Objects(objects) => {
            if let Some(item) = collection(ty) {
                if objects.len() == 1 && objects[0].kind == ty {
                    return Ok(());
                }
                for o in objects {
                    if !compatible(&o.kind, item) {
                        return Err(invalid(format!("expected {item}, got {}", o.kind)));
                    }
                }
            } else {
                if objects.len() != 1 {
                    return Err(invalid(format!("expected one {ty}")));
                }
                for o in objects {
                    if !compatible(&o.kind, ty) {
                        return Err(invalid(format!("expected {ty}, got {}", o.kind)));
                    }
                }
            }
            Ok(())
        }
    }
}
fn validate_text(s: &str, ty: &str) -> Result<()> {
    match ty {
        "String" => {}
        "Double" => {
            numbers::<1>(s)?;
        }
        "Boolean" => {
            boolean(s)?;
        }
        "Point2Double" | "SizeDouble" => {
            numbers::<2>(s)?;
        }
        "RectDouble" => {
            numbers::<4>(s)?;
        }
        "Matrix3x2Double" | "Transform" => {
            tuple_text::<6>(s)?;
        }
        "Geometry" | "PathFigureCollection" => {
            parse_path_markup(s)?;
        }
        "FillRule"
        | "PenLineCap"
        | "PenLineJoin"
        | "GeometryCombineMode"
        | "SweepDirection"
        | "StrokeTransformType" => {
            enumeration(s, ty)?;
        }
        _ => return Err(invalid(format!("{ty} requires object/property elements"))),
    }
    Ok(())
}
fn boolean(s: &str) -> Result<bool> {
    match s.trim() {
        "True" | "true" => Ok(true),
        "False" | "false" => Ok(false),
        _ => Err(invalid("Boolean")),
    }
}
fn enumeration(s: &str, ty: &str) -> Result<u8> {
    let values: &[&str] = match ty {
        "FillRule" => &["EvenOdd", "Nonzero"],
        "PenLineCap" => &["Flat", "Square", "Round", "Triangle"],
        "PenLineJoin" => &["Miter", "Bevel", "Round"],
        "GeometryCombineMode" => &["Union", "Intersect", "Xor", "Exclude"],
        "SweepDirection" => &["Counterclockwise", "Clockwise"],
        "StrokeTransformType" => &["Normal", "Fixed", "Hairline"],
        _ => return Err(invalid(ty)),
    };
    let s = s.trim();
    values
        .iter()
        .position(|v| *v == s)
        .or_else(|| s.parse::<usize>().ok().filter(|i| *i < values.len()))
        .map(|n| n as u8)
        .ok_or_else(|| invalid(format!("{ty}={s}")))
}
fn scalar(v: &Value) -> Result<String> {
    match v {
        Value::Text(s) => Ok(s.clone()),
        Value::Objects(o) if o.len() == 1 => o[0]
            .text
            .clone()
            .ok_or_else(|| invalid("scalar object requires text")),
        _ => Err(invalid("scalar")),
    }
}
impl Object {
    pub fn text(&self, key: &str, default: &str) -> Result<String> {
        self.props
            .get(key)
            .map(scalar)
            .unwrap_or_else(|| Ok(default.into()))
    }
    fn num(&self, key: &str, default: f64) -> Result<f64> {
        self.props
            .get(key)
            .map(|v| numbers::<1>(&scalar(v)?).map(|a| a[0]))
            .unwrap_or(Ok(default))
    }
    fn bool(&self, key: &str, default: bool) -> Result<bool> {
        self.props
            .get(key)
            .map(|v| boolean(&scalar(v)?))
            .unwrap_or(Ok(default))
    }
    fn en(&self, key: &str, ty: &str, default: u8) -> Result<u8> {
        self.props
            .get(key)
            .map(|v| enumeration(&scalar(v)?, ty))
            .unwrap_or(Ok(default))
    }
    fn tuple<const N: usize>(&self, key: &str, default: [f64; N]) -> Result<[f64; N]> {
        let Some(v) = self.props.get(key) else {
            return Ok(default);
        };
        tuple(v)
    }
    fn items(&self, key: &str) -> Result<Vec<&Object>> {
        let Some(v) = self.props.get(key) else {
            return Ok(vec![]);
        };
        match v {
            Value::Objects(o) => {
                if o.len() == 1 && collection(&o[0].kind).is_some() {
                    Ok(o[0].collection_items.iter().collect())
                } else {
                    Ok(o.iter().collect())
                }
            }
            _ => Err(invalid(format!("{key} collection"))),
        }
    }
    fn child(&self, key: &str) -> Option<&Object> {
        match self.props.get(key) {
            Some(Value::Objects(o)) if o.len() == 1 => Some(&o[0]),
            _ => None,
        }
    }
}
fn tuple_text<const N: usize>(s: &str) -> Result<[f64; N]> {
    // The matrix converter accepts surrounding .NET whitespace for this literal only.
    if N == 6 && s.trim() == "Identity" {
        let mut a = [0.; N];
        a.copy_from_slice(&IDENTITY);
        Ok(a)
    } else {
        numbers(s)
    }
}
fn tuple<const N: usize>(v: &Value) -> Result<[f64; N]> {
    if let Value::Objects(o) = v {
        if o.len() != 1 {
            return Err(invalid("value object"));
        }
        let o = &o[0];
        if let Some(s) = &o.text {
            return tuple_text(s);
        }
        let keys: &[&str] = match o.kind.as_str() {
            "Point2Double" => &["X", "Y"],
            "SizeDouble" => &["Width", "Height"],
            "RectDouble" => &["X", "Y", "Width", "Height"],
            "Matrix3x2Double" => &["M11", "M12", "M21", "M22", "OffsetX", "OffsetY"],
            _ => return Err(invalid("value object type")),
        };
        if keys.len() != N {
            return Err(invalid("value dimensions"));
        }
        let mut a = [0.; N];
        if o.kind == "RectDouble" {
            // §9.4.2: Location/Size setters overlap the individual rectangle members.
            for key in &o.member_order {
                match key.as_str() {
                    "Location" => a[..2].copy_from_slice(&o.tuple(key, [0.; 2])?),
                    "Size" => a[2..4].copy_from_slice(&o.tuple(key, [0.; 2])?),
                    _ => {
                        let i = keys.iter().position(|k| *k == key).unwrap();
                        a[i] = o.num(key, 0.)?;
                    }
                }
            }
        } else {
            for (i, k) in keys.iter().enumerate() {
                a[i] = o.num(k, if *k == "M11" || *k == "M22" { 1. } else { 0. })?;
            }
        }
        Ok(a)
    } else {
        tuple_text(&scalar(v)?)
    }
}
fn transform(v: Option<&Value>) -> Result<[f64; 6]> {
    let Some(v) = v else { return Ok(IDENTITY) };
    if let Value::Text(_) = v {
        return tuple(v);
    }
    let Value::Objects(objects) = v else {
        unreachable!()
    };
    let o = &objects[0];
    let mut m = IDENTITY;
    let center = [o.num("CenterX", 0.)?, o.num("CenterY", 0.)?];
    match o.kind.as_str() {
        "IdentityTransform" => {}
        "MatrixTransform" => m = o.tuple("Matrix", IDENTITY)?,
        "TranslateTransform" => {
            m[4] = o.num("X", 0.)?;
            m[5] = o.num("Y", 0.)?;
        }
        "ScaleTransform" => {
            m[0] = o.num("ScaleX", 1.)?;
            m[3] = o.num("ScaleY", 1.)?;
        }
        "RotateTransform" => {
            let (s, c) = o.num("Angle", 0.)?.to_radians().sin_cos();
            m = [c, s, -s, c, 0., 0.];
        }
        "SkewTransform" => {
            m[1] = o.num("AngleY", 0.)?.to_radians().tan();
            m[2] = o.num("AngleX", 0.)?.to_radians().tan();
        }
        "TransformGroup" => {
            for c in o.items("Children")? {
                m = geometry::compose(m, transform(Some(&Value::Objects(vec![c.clone()])))?);
            }
        }
        _ => return Err(invalid("Transform")),
    }
    m[4] += center[0] - m[0] * center[0] - m[2] * center[1];
    m[5] += center[1] - m[1] * center[0] - m[3] * center[1];
    if !m.iter().all(|x| x.is_finite() && x.abs() < 1e100) {
        return Err(invalid("transform overflow"));
    }
    Ok(m)
}
fn stroke(o: Option<&Object>) -> Result<StrokeStyle> {
    let Some(o) = o else {
        return Ok(StrokeStyle::default());
    };
    let mut s = StrokeStyle {
        start: o.en("StartLineCap", "PenLineCap", 0)?,
        end: o.en("EndLineCap", "PenLineCap", 0)?,
        dash_cap: o.en("DashCap", "PenLineCap", 1)?,
        join: o.en("LineJoin", "PenLineJoin", 0)?,
        miter: o.num("MiterLimit", 10.)?,
        ..Default::default()
    };
    if s.miter < 1. || s.miter > f32::MAX as f64 {
        return Err(invalid("MiterLimit"));
    }
    // §9.2 / R93: Fixed and Hairline are treated as Normal only for Widen, pending native capture.
    o.en("TransformType", "StrokeTransformType", 0)?;
    if let Some(d) = o.child("DashStyle") {
        s.offset = d.num("Offset", 0.)?;
        for n in d.items("Dashes")? {
            s.dashes.push(
                numbers::<1>(
                    n.text
                        .as_deref()
                        .ok_or_else(|| invalid("Double needs text"))?,
                )?[0],
            );
        }
        if !s.dashes.len().is_multiple_of(2)
            || s.dashes.iter().any(|d| *d < 0. || *d > f32::MAX as f64)
            || !s.dashes.is_empty() && s.dashes.iter().sum::<f64>() <= 0.
        {
            return Err(invalid(
                "dash pattern must have an even number of nonnegative entries and positive total",
            ));
        }
    }
    Ok(s)
}
fn ellipse(p: &mut Path2D, center: [f64; 2], rx: f64, ry: f64) {
    p.move_to(center[0] + rx, center[1]);
    p.analytic_arc(AnalyticArc {
        center,
        u: [rx, 0.],
        v: [0., ry],
        start: 0.,
        sweep: TAU,
    });
    p.close_path();
}
fn rectangle(p: &mut Path2D, r: [f64; 4], rx: f64, ry: f64) -> Result<()> {
    let [x, y, w, h] = r;
    if w < 0. || h < 0. {
        return Err(invalid("negative rectangle size"));
    }
    let rx = rx.abs().min(w / 2.);
    let ry = ry.abs().min(h / 2.);
    if rx == 0. || ry == 0. {
        p.rect(x, y, w, h);
        return Ok(());
    }
    p.move_to(x + rx, y);
    for (end, center, t) in [
        ([x + w - rx, y], [x + w - rx, y + ry], -PI / 2.),
        ([x + w, y + h - ry], [x + w - rx, y + h - ry], 0.),
        ([x + rx, y + h], [x + rx, y + h - ry], PI / 2.),
        ([x, y + ry], [x + rx, y + ry], PI),
    ] {
        p.line_to(end[0], end[1]);
        p.analytic_arc(AnalyticArc {
            center,
            u: [rx, 0.],
            v: [0., ry],
            start: t,
            sweep: PI / 2.,
        });
    }
    p.close_path();
    Ok(())
}
fn figure(o: &Object, p: &mut Path2D) -> Result<()> {
    let mut current = o.tuple("StartPoint", [0., 0.])?;
    let filled = o.bool("IsFilled", true)?;
    let mut flags = PathFlags {
        filled,
        ..Default::default()
    };
    let i = p.verbs().len();
    p.move_to(current[0], current[1]);
    p.annotate_from(i, flags);
    for seg in o.items("Segments")? {
        flags = PathFlags {
            filled,
            stroked: seg.bool("IsStroked", true)?,
            smooth: seg.bool("IsSmoothJoin", false)?,
        };
        let i = p.verbs().len();
        match seg.kind.as_str() {
            "LineSegment" => {
                current = seg.tuple("Point", [0., 0.])?;
                p.line_to(current[0], current[1]);
            }
            "BezierSegment" => {
                let a = seg.tuple("Point1", [0., 0.])?;
                let b = seg.tuple("Point2", [0., 0.])?;
                current = seg.tuple("Point3", [0., 0.])?;
                p.bezier_curve_to(a[0], a[1], b[0], b[1], current[0], current[1]);
            }
            "QuadraticBezierSegment" => {
                let a = seg.tuple("Point1", [0., 0.])?;
                current = seg.tuple("Point2", [0., 0.])?;
                p.quadratic_curve_to(a[0], a[1], current[0], current[1]);
            }
            "ArcSegment" => {
                let end = seg.tuple("Point", [0., 0.])?;
                geometry::endpoint_arc(
                    p,
                    current,
                    end,
                    seg.tuple("Size", [0., 0.])?,
                    seg.num("RotationAngle", 0.)?,
                    seg.bool("IsLargeArc", false)?,
                    seg.en("SweepDirection", "SweepDirection", 0)? == 1,
                )?;
                current = end;
            }
            "PolyLineSegment"
            | "PolyBezierSegment"
            | "PolyQuadraticBezierSegment"
            | "PolyCurveSegment" => {
                let pts: Vec<[f64; 2]> = seg
                    .items("Points")?
                    .into_iter()
                    .map(|o| tuple(&Value::Objects(vec![o.clone()])))
                    .collect::<Result<_>>()?;
                match seg.kind.as_str() {
                    "PolyLineSegment" => {
                        for v in &pts {
                            p.line_to(v[0], v[1]);
                        }
                    }
                    "PolyBezierSegment" => {
                        if !pts.len().is_multiple_of(3) {
                            return Err(invalid("PolyBezierSegment point count"));
                        }
                        for v in pts.as_chunks::<3>().0 {
                            p.bezier_curve_to(v[0][0], v[0][1], v[1][0], v[1][1], v[2][0], v[2][1]);
                        }
                    }
                    "PolyQuadraticBezierSegment" => {
                        if !pts.len().is_multiple_of(2) {
                            return Err(invalid("PolyQuadraticBezierSegment point count"));
                        }
                        for v in pts.as_chunks::<2>().0 {
                            p.quadratic_curve_to(v[0][0], v[0][1], v[1][0], v[1][1]);
                        }
                    }
                    _ => {
                        let mut knots = vec![current];
                        knots.extend_from_slice(&pts);
                        for [a, b, c] in super::super::shapes::natural_spline(&knots) {
                            p.bezier_curve_to(a[0], a[1], b[0], b[1], c[0], c[1]);
                        }
                    }
                }
                if let Some(last) = pts.last() {
                    current = *last;
                }
            }
            _ => return Err(invalid("path segment")),
        }
        p.annotate_from(i, flags);
        if p.verbs().len() > MAX_SEGMENTS {
            return Err(invalid("geometry segment limit"));
        }
    }
    if o.bool("IsClosed", false)? {
        let i = p.verbs().len();
        p.close_path();
        p.annotate_from(i, flags);
    }
    Ok(())
}
pub(super) fn geometry_value(v: &Value) -> Result<Geometry> {
    match v {
        Value::Text(s) => {
            let (path, rule) = parse_path_markup(s)?;
            Ok(Geometry {
                path,
                rule,
                infinite: false,
            })
        }
        Value::Objects(o) if o.len() == 1 => geometry_object(&o[0]),
        _ => Err(invalid("Geometry")),
    }
}
// R99: only the direct root InfiniteGeometry has the port-only no-render policy.
fn reject_nested_infinite(g: &Geometry) -> Result<()> {
    if g.infinite {
        Err(XamlError::NotAllowed(
            "InfiniteGeometry inside a geometry operation".into(),
        ))
    } else {
        Ok(())
    }
}
fn geometry_object(o: &Object) -> Result<Geometry> {
    let mut g = Geometry::default();
    let t = transform(o.props.get("Transform"))?;
    let mut already_transformed = false;
    match o.kind.as_str() {
        "EmptyGeometry" => {}
        "InfiniteGeometry" => g.infinite = true,
        "LineGeometry" => {
            let a = o.tuple("StartPoint", [0., 0.])?;
            let b = o.tuple("EndPoint", [0., 0.])?;
            g.path.move_to(a[0], a[1]);
            g.path.line_to(b[0], b[1]);
        }
        "EllipseGeometry" => ellipse(
            &mut g.path,
            o.tuple("Center", [0., 0.])?,
            o.num("RadiusX", 0.)?.abs(),
            o.num("RadiusY", 0.)?.abs(),
        ),
        "RectangleGeometry" => rectangle(
            &mut g.path,
            o.tuple("Rect", [0.; 4])?,
            o.num("RadiusX", 0.)?,
            o.num("RadiusY", 0.)?,
        )?,
        "PathGeometry" => {
            if let Some(Value::Text(s)) = o.props.get("Figures") {
                g.path = parse_path_markup(s)?.0;
            } else if let Some(s) = o.child("Figures").and_then(|o| o.text.as_ref()) {
                g.path = parse_path_markup(s)?.0;
            } else {
                for f in o.items("Figures")? {
                    figure(f, &mut g.path)?
                }
            }
            g.rule = if o.en("FillRule", "FillRule", 0)? == 0 {
                FillRule::EvenOdd
            } else {
                FillRule::NonZero
            };
        }
        "GeometryGroup" => {
            for child in o.items("Children")? {
                let c = geometry_object(child)?;
                reject_nested_infinite(&c)?;
                g.path.push_mapped(&c.path, &IDENTITY);
                if g.path.verbs().len() > MAX_SEGMENTS {
                    return Err(invalid("geometry segment limit"));
                }
            }
            g.rule = if o.en("FillRule", "FillRule", 0)? == 0 {
                FillRule::EvenOdd
            } else {
                FillRule::NonZero
            };
        }
        "CombinedGeometry" => {
            let a = o
                .props
                .get("Geometry1")
                .map(geometry_value)
                .transpose()?
                .unwrap_or_default();
            let b = o
                .props
                .get("Geometry2")
                .map(geometry_value)
                .transpose()?
                .unwrap_or_default();
            reject_nested_infinite(&a)?;
            reject_nested_infinite(&b)?;
            let tol = o.num("FlatteningTolerance", 0.25)? as f32 as f64;
            let mode = match o.en("GeometryCombineMode", "GeometryCombineMode", 0)? {
                0 => CombineMode::Union,
                1 => CombineMode::Intersect,
                2 => CombineMode::Xor,
                _ => CombineMode::Exclude,
            };
            g.path = geometry::combine(&a, &b, mode, tol)?;
            g.rule = FillRule::NonZero;
        }
        "TransformedGeometry" | "FlattenedGeometry" | "OutlinedGeometry" | "WidenedGeometry" => {
            g = o
                .props
                .get("Geometry")
                .map(geometry_value)
                .transpose()?
                .unwrap_or_default();
            reject_nested_infinite(&g)?;
            let tol = o.num("FlatteningTolerance", 0.25)?.max(1e-6) as f32 as f64;
            match o.kind.as_str() {
                "FlattenedGeometry" => {
                    g.path = geometry::flatten(
                        &geometry::mapped(&g.path, t),
                        tol,
                        o.bool("AllowCurves", false)?,
                    )?;
                    already_transformed = true;
                }
                "OutlinedGeometry" => {
                    g.path = geometry::outline(
                        &Geometry {
                            path: geometry::mapped(&g.path, t),
                            ..g
                        },
                        tol,
                    )?;
                    g.rule = FillRule::NonZero;
                    already_transformed = true;
                }
                "WidenedGeometry" => {
                    let thickness = o.num("Thickness", 1.)?;
                    g.path = geometry::widen(
                        &g.path,
                        thickness,
                        &stroke(o.child("StrokeStyle"))?,
                        tol,
                        t,
                    )?;
                    if thickness <= 0. {
                        g.infinite = false;
                    }
                    g.rule = FillRule::NonZero;
                    already_transformed = true;
                }
                _ => {}
            }
        }
        _ => return Err(invalid("geometry object type")),
    }
    if !already_transformed {
        g.path = geometry::mapped(&g.path, t)
    }
    if g.infinite {
        g.path = Path2D::new()
    }
    let b = geometry::path_bounds(&g.path);
    if ![b.x, b.y, b.width, b.height]
        .iter()
        .all(|x| x.is_finite() && x.abs() < 1e100)
    {
        return Err(invalid("geometry overflow"));
    }
    Ok(g)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matrix_form(form: usize, text: &str) -> Result<[f64; 6]> {
        let content = match form {
            0 => format!(r#" Matrix="{text}"/>"#),
            1 => format!(
                "><MatrixTransform.Matrix>{text}</MatrixTransform.Matrix></MatrixTransform>"
            ),
            2 => format!(
                "><MatrixTransform.Matrix><p:Matrix3x2Double>{text}</p:Matrix3x2Double></MatrixTransform.Matrix></MatrixTransform>"
            ),
            _ => unreachable!(),
        };
        let source = format!(r#"<MatrixTransform xmlns="{MEDIA}" xmlns:p="{PRIMITIVES}"{content}"#);
        let doc = roxmltree::Document::parse(&source).map_err(|e| invalid(e.to_string()))?;
        transform(Some(&Value::Objects(vec![object(doc.root_element(), 0)?])))
    }

    // Typed initialization follows the supplied converter analysis; it is not a native run.
    #[test]
    fn matrix_identity_all_construction_forms() {
        let whitespace = [
            "", " ", "\t\r\n", "&#x85;", "&#xA0;", "&#x1680;", "&#x2000;", "&#x2028;", "&#x2029;",
            "&#x202F;", "&#x205F;", "&#x3000;",
        ];
        for form in 0..3 {
            for space in whitespace {
                let text = format!("{space}Identity{space}");
                assert_eq!(
                    matrix_form(form, &text).unwrap(),
                    [1., 0., 0., 1., 0., 0.],
                    "form {form}: {text:?}"
                );
            }
        }
    }

    #[test]
    fn matrix_identity_typed_initialization() {
        assert_eq!(
            matrix_form(2, "Identity").unwrap(),
            [1., 0., 0., 1., 0., 0.]
        );
    }

    #[test]
    fn matrix_identity_rejects_extra_tokens_and_lookalikes() {
        for form in 0..3 {
            for text in [
                "identity",
                "IDENTITY",
                "Identity 0",
                "IdentityIdentity",
                ",Identity",
                "Identity,",
                "Identity,,",
                "Identity;",
                ";Identity",
                "Identity;0",
                "Iden tity",
                "&#x180E;Identity",
                "Identity&#x200B;",
                "&#xFEFF;Identity",
            ] {
                assert!(matrix_form(form, text).is_err(), "form {form}: {text:?}");
            }
        }
    }

    #[test]
    fn matrix_identity_non_xml_whitespace() {
        // XML 1.0 forbids VT/FF and these control lookalikes; test the converter directly.
        let text = "\x0bIdentity\x0c";
        validate_text(text, "Matrix3x2Double").unwrap();
        assert_eq!(
            tuple::<6>(&Value::Text(text.into())).unwrap(),
            [1., 0., 0., 1., 0., 0.]
        );
        for text in ["\x1cIdentity", "Identity\x1f", "Identity\u{200b}"] {
            assert!(validate_text(text, "Matrix3x2Double").is_err());
            assert!(tuple::<6>(&Value::Text(text.into())).is_err());
        }
    }

    #[test]
    fn matrix_numeric_all_construction_forms() {
        for form in 0..3 {
            for (text, expected) in [
                ("1,0,0,1,0,0", [1., 0., 0., 1., 0., 0.]),
                ("2,1,-1,3,5,7", [2., 1., -1., 3., 5., 7.]),
            ] {
                assert_eq!(matrix_form(form, text).unwrap(), expected);
            }
        }
    }
}
