//! Oracle for genuine `GaussianBlurEffect2`'s inner D2D `GaussianBlur` on opend2d's d2d executor
//! (8bpc premultiplied node buffers, == native WARP). Feeds a PREMULTIPLIED BGRA bitmap (the 8bpc
//! premultiplied linear node the port's blurEffect produces at the CBlur boundary, dumped via
//! `PDNGPU_CBLUR`), runs `CLSID_D2D1GaussianBlur` with BorderMode.Hard, dumps the premultiplied BGRA
//! output for a byte-compare against the port's `cblur::gaussian_blur_premul`.
//! Usage: `pdngpu-gaussiangraph <sigma> <optimization> <in.premul.bgra> <out.bgra> <W> <H>`.
//! Prop indices (D2D1_GAUSSIANBLUR_PROP, from the executor's kind_of/validation): 0 StandardDeviation
//! (f32), 1 Optimization (enum), 2 BorderMode (enum; 1 = Hard).
use d2d::abi::{CLSID_D2D1GaussianBlur, D2D1_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM};
use d2d::bitmap::Bitmap;
use d2d::effects::{Input, Prop};
use d2d::{bitmap_target, Factory};
use std::rc::Rc;

fn main() {
    let a: Vec<_> = std::env::args().collect();
    let sigma: f32 = a[1].parse().expect("sigma");
    let mode: u32 = a[2].parse().expect("optimization");
    let in_bgra = std::fs::read(&a[3]).expect("in.premul.bgra");
    let out = &a[4];
    let (w, h): (u32, u32) = (a[5].parse().unwrap(), a[6].parse().unwrap());
    assert_eq!(in_bgra.len(), (w * h * 4) as usize, "input size");

    let factory = Factory::new(false);
    let device = factory.create_device();
    let mut dc = device.create_device_context();
    dc.set_dpi(96.0, 96.0);
    dc.set_target_bitmap(bitmap_target(w, h, 96.0));
    dc.begin_draw();
    dc.clear(Some([0.0; 4]));

    // Feed the premultiplied node bytes verbatim (realize_input clones a Bitmap input as-is).
    let bm = Bitmap {
        width: w,
        height: h,
        premul_bgra: in_bgra,
        format: DXGI_FORMAT_B8G8R8A8_UNORM,
        alpha: D2D1_ALPHA_MODE_PREMULTIPLIED,
        dpi: (96.0, 96.0),
    };
    let blur = dc.create_effect(CLSID_D2D1GaussianBlur).expect("GaussianBlur");
    blur.set_value(0, Prop::F32(sigma)).unwrap(); // StandardDeviation (px)
    blur.set_value(1, Prop::U32(mode)).unwrap(); // Optimization (0 SPEED / 1 BALANCED / 2 QUALITY)
    blur.set_value(2, Prop::U32(1)).unwrap(); // BorderMode = Hard (clamp)
    blur.set_input(0, Input::Bitmap(Rc::new(bm)));

    dc.draw_image_effect(&blur, [0.0, 0.0], 1, 0);
    let _ = dc.end_draw();
    let data = dc.take_target().unwrap().data;
    std::fs::write(out, &data).expect("write out.bgra");
    eprintln!("rendered executor gaussian (sigma={sigma}, opt={mode}, hard) -> {out} ({} bytes)", data.len());
}
