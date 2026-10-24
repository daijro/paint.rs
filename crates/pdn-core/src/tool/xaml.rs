//! Paint.NET's shape XAML (R91–R93).
//! Catalogue/record structure: paintdotjs tool/shape/{ShapeCatalog,ShapeManager}.js.
//! This is a bounded ordinary-member XAML reader, not a System.Xaml interpreter.
//! Product limits: 512 KiB source, 80 UTF-16 name units, 32768 XML nodes, depth 64,
//! 65536 path/subdivision segments, and boolean inputs of at most 4096 edges and
//! 65536 overlapping edge-box pairs. Operations fail rather than relax tolerance.
//! Native Widen uses the R93 f32 clipped-miter/cap/dash approximation; hollow fit
//! bounds use stroked centerlines (R92). R99 accepts only a direct Infinite root
//! as a no-render placeholder and rejects Infinite inside operations.
mod geometry;
mod markup;
mod xml;
use super::shapes::{DashStyle, DrawType, ShapeRenderData, fit_to_rect};
use crate::{
    canvas::{FillRule, Path2D},
    util::geom::Rectangle,
};
pub use geometry::path_bounds;
use geometry::{Geometry, IDENTITY};
pub use markup::parse_path_markup;

#[derive(Clone, Debug, PartialEq)]
pub struct ParsedShape {
    pub name: String,
    pub geometry: Path2D,
    pub fill_rule: FillRule,
    pub bounds: Rectangle,
    infinite: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum XamlError {
    Doctype,
    TooLarge,
    NameTooLong,
    NotXml(String),
    Root(String),
    NotAllowed(String),
    MissingGeometry,
    BadAttribute(String),
    BadPathData(String),
}
impl std::fmt::Display for XamlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let msg = match self {
            Self::Doctype => "DTD/entity declarations are forbidden".into(),
            Self::TooLarge => "Shape source exceeds 512 KB".into(),
            Self::NameTooLong => "Shape name exceeds 80 characters".into(),
            Self::MissingGeometry => "Shape has no Geometry".into(),
            Self::NotXml(s) => format!("Invalid XML: {s}"),
            Self::Root(s) => format!("Invalid shape root: {s}"),
            Self::NotAllowed(s) => format!("Unsupported XAML: {s}"),
            Self::BadAttribute(s) => format!("Invalid property: {s}"),
            Self::BadPathData(s) => format!("Invalid path: {s}"),
        };
        write!(f, "{}", msg.replace(['\r', '\n'], " "))
    }
}
impl std::error::Error for XamlError {}
type Result<T> = std::result::Result<T, XamlError>;
const MAX_SEGMENTS: usize = 65_536;
fn invalid(s: impl Into<String>) -> XamlError {
    XamlError::BadAttribute(s.into())
}

pub fn parse(source: &str) -> Result<ParsedShape> {
    if source.len() > 512 * 1024 {
        return Err(XamlError::TooLarge);
    }
    if source.contains("<!DOCTYPE") || source.contains("<!ENTITY") {
        return Err(XamlError::Doctype);
    }
    let root = xml::read(source)?;
    if root.kind != "SimpleGeometryShape" {
        return Err(XamlError::Root(root.kind));
    }
    let name = root.text("DisplayName", "")?;
    if name.encode_utf16().count() > 80 {
        return Err(XamlError::NameTooLong);
    }
    let name = if name.trim().is_empty() {
        "SimpleGeometryShapeWrapper".into()
    } else {
        name
    };
    let value = root
        .props
        .get("Geometry")
        .ok_or(XamlError::MissingGeometry)?;
    let g = xml::geometry_value(value)?;
    let bounds = if g.infinite {
        Rectangle::default()
    } else {
        path_bounds(&g.path)
    };
    Ok(ParsedShape {
        name,
        geometry: g.path,
        fill_rule: g.rule,
        bounds,
        infinite: g.infinite,
    })
}

#[derive(Clone, Debug, Default)]
pub struct ShapeCatalog {
    pub custom: Vec<(String, ParsedShape)>,
}
impl ShapeCatalog {
    pub fn register(&mut self, id: String, shape: ParsedShape) {
        if let Some((_, old)) = self.custom.iter_mut().find(|(key, _)| key == &id) {
            *old = shape
        } else {
            self.custom.push((id, shape));
        }
    }
    pub fn get(&self, id: &str) -> Option<&ParsedShape> {
        self.custom
            .iter()
            .find(|(key, _)| key == id)
            .map(|(_, s)| s)
    }
    pub(crate) fn selected(&self, value: &str) -> Option<&ParsedShape> {
        self.get(value.strip_prefix("custom.")?)
    }
    pub fn create_path(&self, id: &str, rect: [f64; 4]) -> Option<Path2D> {
        Some(self.get(id)?.fitted(rect))
    }
    pub fn render_data(
        &self,
        id: &str,
        rect: [f64; 4],
        width: f64,
        draw: DrawType,
        dash: DashStyle,
    ) -> Result<Option<ShapeRenderData>> {
        self.get(id)
            .map(|s| s.render_data(rect, width, draw, dash))
            .transpose()
    }
    pub fn values(&self) -> Vec<String> {
        super::shapes::ShapeKind::ALL
            .iter()
            .map(|s| s.value().into())
            .chain(self.custom.iter().map(|(id, _)| format!("custom.{id}")))
            .collect()
    }
}
impl ParsedShape {
    pub fn aspect_ratio(&self) -> f64 {
        if self.bounds.width == 0. || self.bounds.height == 0. {
            1.
        } else {
            self.bounds.width / self.bounds.height
        }
    }
    fn fitted(&self, r: [f64; 4]) -> Path2D {
        if self.infinite || r[2] == 0. || r[3] == 0. {
            return Path2D::new();
        }
        let b = &self.bounds;
        fit_to_rect(&self.geometry, [b.x, b.y, b.right(), b.bottom()], r)
    }
    pub(crate) fn render_data(
        &self,
        r: [f64; 4],
        width: f64,
        draw: DrawType,
        dash: DashStyle,
    ) -> Result<ShapeRenderData> {
        let p = self.fitted(r);
        let mut d = ShapeRenderData::default();
        if draw != DrawType::Outline {
            d.interior = Some((geometry::filled(&p), self.fill_rule));
        }
        if draw != DrawType::Interior {
            let style = geometry::StrokeStyle::toolbar(width, dash);
            d.outline = Some((
                geometry::widen(&p, width, &style, 0.1, IDENTITY)?,
                FillRule::NonZero,
            ));
            d.tracer = Some(geometry::stroke_polylines(&p, 0.25)?);
        }
        Ok(d)
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct ShapeRecord {
    pub id: String,
    pub file_name: String,
    pub name: String,
    pub size: u64,
    pub source: String,
    pub imported_at: f64,
    pub updated_at: f64,
}
