//! Oracle for the GENUINE PdnEmbossEffect built-in graph on opend2d's d2d executor (8bpc
//! premultiplied intermediate node buffers, as native WARP): Grayscale -> ConvolveMatrix{3x3 kernel,
//! PreserveAlpha, BorderMode.Hard} -> ColorMatrix(identity + 128/255 rgb offset). Dumps 32x32 BGRA8.
//! Usage: `pdngpu-embossgraph <angle-deg> <in.bgra> <out.bgra> [W H]`.
//! The 3x3 kernel is genuine's `cos(num + k·π/4)` with num=(angle−180)·2π/360, computed in f32 via
//! crt::cosf (== ucrtbase cosf == MathF.Cos), UNFLIPPED row-major (the executor flips for convolution).
use d2d::abi::{CLSID_D2D1ColorMatrix, CLSID_D2D1ConvolveMatrix, CLSID_D2D1Grayscale};
use d2d::bitmap::Bitmap;
use d2d::effects::{Input, Prop};
use d2d::{bitmap_target, Factory};
use std::rc::Rc;

fn main() {
    let a: Vec<_> = std::env::args().collect();
    let angle: f32 = a[1].parse().expect("angle-deg");
    let num = (angle - 180.0) * 2.0 * std::f32::consts::PI / 360.0;
    let num2 = std::f32::consts::PI / 4.0;
    let c = |k: f32| crt::cosf(num + k * num2);
    // genuine SetKernelMatrixAndSize order (3x3 row-major, centre 0):
    let kernel: Vec<f32> = vec![
        c(1.0), c(2.0), c(3.0),
        c(0.0), 0.0, c(4.0),
        c(-1.0), c(-2.0), c(-3.0),
    ];
    let in_bgra = std::fs::read(&a[2]).expect("in.bgra");
    let out = &a[3];
    let (w, h): (u32, u32) = (a.get(4).map_or(32, |s| s.parse().unwrap()), a.get(5).map_or(32, |s| s.parse().unwrap()));
    let rgba: Vec<u8> = in_bgra.chunks_exact(4).flat_map(|p| [p[2], p[1], p[0], p[3]]).collect();

    let factory = Factory::new(false);
    let device = factory.create_device();
    let mut dc = device.create_device_context();
    dc.set_dpi(96.0, 96.0);
    dc.set_target_bitmap(bitmap_target(w, h, 96.0));
    dc.begin_draw();
    dc.clear(Some([0.0; 4]));

    let gray = dc.create_effect(CLSID_D2D1Grayscale).expect("Grayscale");
    gray.set_input(0, Input::Bitmap(Rc::new(Bitmap::from_straight_rgba(w, h, &rgba))));

    let conv = dc.create_effect(CLSID_D2D1ConvolveMatrix).expect("ConvolveMatrix");
    conv.set_value(2, Prop::U32(3)).unwrap(); // KernelSizeX
    conv.set_value(3, Prop::U32(3)).unwrap(); // KernelSizeY
    conv.set_value(4, Prop::Floats(kernel.into())).unwrap(); // KernelMatrix (unflipped)
    conv.set_value(8, Prop::Bool(true)).unwrap(); // PreserveAlpha
    conv.set_value(9, Prop::U32(1)).unwrap(); // BorderMode.Hard (= clamp)
    conv.set_input(0, Input::Effect(gray));

    // identity 4x4 + offset row (+128/255 on rgb), premultiplied alpha mode (opaque input -> straight).
    let o = 128.0f32 / 255.0;
    #[rustfmt::skip]
    let m = [1.0,0.0,0.0,0.0, 0.0,1.0,0.0,0.0, 0.0,0.0,1.0,0.0, 0.0,0.0,0.0,1.0, o,o,o,0.0];
    let cm = dc.create_effect(CLSID_D2D1ColorMatrix).expect("ColorMatrix");
    cm.set_value(0, Prop::M54(m)).unwrap();
    cm.set_value(1, Prop::U32(1)).unwrap(); // alpha mode: 1 = premultiplied
    cm.set_input(0, Input::Effect(conv));

    dc.draw_image_effect(&cm, [0.0, 0.0], 1, 0);
    let _ = dc.end_draw();
    let data = dc.take_target().unwrap().data;
    std::fs::write(out, &data).expect("write out.bgra");
    eprintln!("rendered genuine emboss graph -> {out} ({} bytes)", data.len());
}
