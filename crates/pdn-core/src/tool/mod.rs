//! Tools: types, settings and option definitions (Task 17); the `Tool` trait every tool implements
//! (Task 21); Task 24 adds the tool base and the tools themselves.
//! Port of: js/src/engine/document/tool/Tool.js (the hooks AppView/AppWorkspace call)
//! Reference: paintdotjs @ b4e6fc5
pub mod base;
pub mod brush;
pub mod drawing;
pub mod fill;
pub mod flood;
pub mod gradient;
pub mod healing;
pub mod magnetic_lasso;
pub mod move_render;
pub mod move_tools;
pub mod object_select;
pub mod pan_zoom;
pub mod patch;
pub mod perspective_warp;
pub mod picker;
pub mod remove;
pub mod selection_tools;
pub mod shapes;
pub mod spot_healing;
pub mod text;
pub mod transacted;
pub mod types;
pub mod xaml;

pub use base::ToolBase;

use crate::app::AppWorkspace;
pub use crate::util::shortcut::MouseButton;
pub use types::ToolType;

/// A tool's hooks, called by `AppWorkspace` with the take-and-restore pattern (the tool is out of
/// `app.tool` while it runs). Mouse coordinates are document coordinates.
///
/// All five physical buttons reach the current gesture, including chord presses/releases.
/// Tools keep their own release rule: brush and picker drags end on any release
/// (painting-tools-1.md §8.7); selection/move tools retain their origin/owner-button rules
/// (`selection-and-move.md §2.5` step 3).
pub trait Tool {
    fn tool_type(&self) -> ToolType;
    fn on_activate(&mut self, _app: &mut AppWorkspace) {}
    fn on_deactivate(&mut self, _app: &mut AppWorkspace) {}
    fn on_mouse_down(
        &mut self,
        _app: &mut AppWorkspace,
        _x: f64,
        _y: f64,
        _b: MouseButton,
        _i: &PointerInput,
    ) -> bool {
        false
    }
    fn on_mouse_move(
        &mut self,
        _app: &mut AppWorkspace,
        _x: f64,
        _y: f64,
        _i: &PointerInput,
    ) -> bool {
        false
    }
    fn on_mouse_up(
        &mut self,
        _app: &mut AppWorkspace,
        _x: f64,
        _y: f64,
        _b: MouseButton,
        _i: &PointerInput,
    ) -> bool {
        false
    }
    fn on_key_press(&mut self, _app: &mut AppWorkspace, _key: &KeyPress) -> bool {
        false
    }
    fn on_modifier_keys_changed(&mut self, _app: &mut AppWorkspace) {}
    fn on_selection_changing(&mut self, _app: &mut AppWorkspace) {}
    fn on_selection_changed(&mut self, _app: &mut AppWorkspace) {}
    fn on_executing_history_memento(&mut self, _app: &mut AppWorkspace) {}
    fn on_executed_history_memento(&mut self, _app: &mut AppWorkspace) {}
    fn on_colors_changed(&mut self, _app: &mut AppWorkspace) {}
    fn on_setting_changed(&mut self, _app: &mut AppWorkspace, _key: &str) {}
    /// Called once per UI frame and after every harness event (Task 24).
    fn on_frame(&mut self, _app: &mut AppWorkspace) {}
    fn commit_pending(&mut self, _app: &mut AppWorkspace) -> bool {
        false
    }
    /// Renders a drag's float (`DocumentWorkspace::float`) into the layer, so what reads the
    /// layer sees the pixels; the drag goes on (its next frame lifts them again).
    fn land_float(&mut self, _app: &mut AppWorkspace) {}
    /// The toolbar's Action option `key` was clicked (S§4.3: Object Selection's Select
    /// Subject); Finish, which has no key, calls `commit_pending`.
    fn on_action(&mut self, _app: &mut AppWorkspace, _key: &str) {}
    fn uses_continuous_pointer_coordinates(&self) -> bool {
        false
    }
    /// A transacted tool's core (painting-tools-2.md §7): the app steps its history entries and
    /// sets its transaction aside through it.
    fn transacted(&mut self) -> Option<&mut dyn transacted::DynTransacted> {
        None
    }
    /// A timer of the tool's own is running (the move tools' nudge burst, which commits after
    /// 750 ms without a key): the UI keeps frames coming for `on_frame`, as for a transacted
    /// tool's settings window.
    fn needs_ticks(&self) -> bool {
        false
    }
    /// A background job of the tool's runs (`extra::job`, S§4.3): the canvas shows the busy
    /// cursor.
    fn busy(&self) -> bool {
        false
    }
    /// The status bar's selection angle while a move transaction is open
    /// (`ui-drawn-graphics.md §2.10` "Selection info"); `None` for every other tool.
    fn status_text(&self) -> Option<&str> {
        None
    }
    fn status_angle(&self) -> Option<f64> {
        None
    }
    /// S§4.6 (Extra): Move Selected Pixels is translating (a drag or a nudge burst), so the status
    /// bar offers Shift's trail; `false` for every other tool.
    fn shift_trail_hint(&self) -> bool {
        false
    }
    fn cursor(&self) -> CursorRequest {
        CursorRequest::Css("default")
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}

/// Canvas coordinates of the device pixel under the pointer (its top-left corner); tools apply their
/// own Paint.NET rule (brush tools add hw/2, selection tools floor, Line snaps to pixel centres).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointerSample {
    pub x: f64,
    pub y: f64,
    /// None: no pressure was reported (mouse, or a pen without pressure); Task 36 fills it.
    pub pressure: Option<f64>,
    pub time_ms: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PointerInput {
    pub pointer_type: PointerType,
    pub samples: Vec<PointerSample>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerType {
    Mouse,
    Pen,
    Touch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyPress {
    pub key: String,
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
}

/// What the canvas shows as the pointer: a CSS `cursor` keyword, a PNG from `assets/cursors`
/// (name without extension), or nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorRequest {
    Css(&'static str),
    Image(&'static str),
    None,
}

/// `ToolType.create()` for the tools ported so far (`None` for the rest until their tasks).
pub fn create_tool(tool_type: ToolType) -> Option<Box<dyn Tool>> {
    use selection_tools::{EllipseSelectTool, LassoSelectTool, RectangleSelectTool};
    Some(match tool_type {
        ToolType::Text => Box::<text::TextTool>::default(),
        ToolType::Shapes => Box::<shapes::ShapesTool>::default(),
        ToolType::Line => Box::<shapes::LineTool>::default(),
        ToolType::Pan => Box::<pan_zoom::PanTool>::default(),
        ToolType::RectangleSelect => Box::<RectangleSelectTool>::default(),
        ToolType::EllipseSelect => Box::<EllipseSelectTool>::default(),
        ToolType::LassoSelect => Box::<LassoSelectTool>::default(),
        ToolType::PaintBucket => Box::<flood::PaintBucketTool>::default(),
        ToolType::Gradient => Box::<gradient::GradientTool>::default(),
        ToolType::MagicWand => Box::<flood::MagicWandTool>::default(),
        ToolType::Move => Box::<move_tools::MoveTool>::default(),
        ToolType::MoveSelection => Box::<move_tools::MoveSelectionTool>::default(),
        ToolType::PaintBrush => Box::<drawing::PaintBrushTool>::default(),
        ToolType::Eraser => Box::<drawing::EraserTool>::default(),
        ToolType::Pencil => Box::<drawing::PencilTool>::default(),
        ToolType::CloneStamp => Box::<drawing::CloneStampTool>::default(),
        ToolType::Recolor => Box::<drawing::RecolorTool>::default(),
        ToolType::ColorPicker => Box::<picker::ColorPickerTool>::default(),
        ToolType::Zoom => Box::<pan_zoom::ZoomTool>::default(),
        ToolType::Remove => Box::<remove::RemoveTool>::default(),
        ToolType::MagneticLasso => Box::<magnetic_lasso::MagneticLassoTool>::default(),
        ToolType::ObjectSelection => Box::<object_select::ObjectSelectionTool>::default(),
        ToolType::HealingBrush => Box::<healing::HealingBrushTool>::default(),
        ToolType::SpotHealingBrush => Box::<spot_healing::SpotHealingBrushTool>::default(),
        ToolType::Patch => Box::<patch::PatchTool>::default(),
    })
}
