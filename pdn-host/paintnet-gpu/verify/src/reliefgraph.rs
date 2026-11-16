//! Oracle for GENUINE PdnReliefEffect on opend2d's d2d executor: ConvolveMatrix{3x3 relief kernel,
//! PreserveAlpha=false, BorderMode.Hard} on the (premultiplied) input — no grayscale, no offset.
//! Kernel = cos(num + k·π/4) with num = angle·2π/360, centre [1,1]=1, via crt::cosf (== MathF.Cos).
//! Usage: `pdngpu-reliefgraph <angle-deg> <in.bgra> <out.bgra> [W H]`. Feed opaque (premul==straight).
use d2d::abi::CLSID_D2D1ConvolveMatrix;
use d2d::bitmap::Bitmap;
use d2d::effects::{Input, Prop};
use d2d::{bitmap_target, Factory};
use std::rc::Rc;

fn main() {
    let a: Vec<_> = std::env::args().collect();
    let angle: f32 = a[1].parse().expect("angle-deg");
    let num = angle * (std::f32::consts::PI * 2.0) / 360.0;
    let d = std::f32::consts::PI / 4.0;
    let c = |k: f32| crt::cosf(num + k * d);
    // genuine array (row-major), centre 1 — the executor flips it internally for convolution.
    let kernel: Vec<f32> = vec![
        c(-3.0), c(-2.0), c(-1.0),
        c(4.0), 1.0, c(0.0),
        c(3.0), c(2.0), c(1.0),
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

    let conv = dc.create_effect(CLSID_D2D1ConvolveMatrix).expect("ConvolveMatrix");
    conv.set_value(2, Prop::U32(3)).unwrap();
    conv.set_value(3, Prop::U32(3)).unwrap();
    conv.set_value(4, Prop::Floats(kernel.into())).unwrap();
    conv.set_value(8, Prop::Bool(false)).unwrap(); // PreserveAlpha = false
    conv.set_value(9, Prop::U32(1)).unwrap(); // BorderMode.Hard
    conv.set_input(0, Input::Bitmap(Rc::new(Bitmap::from_straight_rgba(w, h, &rgba))));

    dc.draw_image_effect(&conv, [0.0, 0.0], 1, 0);
    let _ = dc.end_draw();
    let data = dc.take_target().unwrap().data;
    std::fs::write(out, &data).expect("write out.bgra");
    eprintln!("rendered genuine relief graph -> {out} ({} bytes)", data.len());
}
