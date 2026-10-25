use super::*;
use crate::extra::interrupt::Interrupt;
use crate::extra::segment::{grabcut, subject};
use candle_core::Module;

/// A 2048×1024 canvas whose frame scale is ½: the logits' positive cells land on the matching
/// document pixels, and the background rule counts border cells.
#[test]
fn logits_map_to_document_pixels() {
    let e = wide();
    // Cells 10..20 × 30..40 positive: frame px 40..80 × 120..160, document 80..160 × 240..320.
    let mut logits = vec![-5.0f32; LOW * LOW];
    for r in 30..40 {
        for c in 10..20 {
            logits[r * LOW + c] = 5.0;
        }
    }
    let r = region(&logits, &e, e.canvas).unwrap();
    let full = |x: i32, y: i32| r.at(x, y) == 1.0;
    assert!(full(100 + 85, 50 + 245) && full(100 + 155, 50 + 315));
    assert_eq!(r.at(100 + 70, 50 + 280), 0.0);
    assert_eq!(r.at(100 + 120, 50 + 330), 0.0);
    let b = r.bbox;
    assert!((b.x0 - 180).abs() <= 6 && (b.x1 - 260).abs() <= 6, "{b:?}");
    assert!((b.y0 - 290).abs() <= 6 && (b.y1 - 370).abs() <= 6, "{b:?}");
    assert!(!is_background(&logits, &e));
    // Clipped to a box.
    let clip = Rect::new(200, 0, 3000, 3000);
    assert!(region(&logits, &e, clip).unwrap().bbox.x0 >= 200);
    // Positive everywhere in the scaled image: background.
    assert!(is_background(&vec![1.0; LOW * LOW], &e));
}

/// A deterministic tensor of `shape`, values in -1..1 (a hash of the index and `seed`).
fn sample(shape: &[usize], seed: u32) -> Tensor {
    let n = shape.iter().product::<usize>();
    let v = (0..n as u32)
        .map(|i| {
            let h = (i ^ seed.wrapping_mul(0x9e37_79b9)).wrapping_mul(0x85eb_ca6b);
            let h = (h ^ (h >> 13)).wrapping_mul(0xc2b2_ae35);
            (h ^ (h >> 16)) as f32 / u32::MAX as f32 * 2.0 - 1.0
        })
        .collect();
    Tensor::from_vec(v, shape, &Device::Cpu).unwrap()
}

/// The largest difference between two tensors of the same shape.
fn max_diff(a: &Tensor, b: &Tensor) -> f32 {
    assert_eq!(a.dims(), b.dims());
    (a - b)
        .unwrap()
        .abs()
        .unwrap()
        .max_all()
        .unwrap()
        .to_scalar()
        .unwrap()
}

/// The encoder's folded batch norms match candle's convolution then batch norm (eval mode, eps
/// 1e-5): dense, depthwise (the vendored kernel) and grouped (candle's), strides 1 and 2.
#[test]
fn folded_batch_norm_matches_conv_then_batch_norm() {
    use candle_nn::{BatchNorm, Conv2dConfig};
    for (in_, out, groups) in [(4, 6, 1), (6, 6, 6), (4, 6, 2)] {
        for stride in [1, 2] {
            let seed = (in_ * 100 + groups * 10 + stride) as u32;
            let w = sample(&[out, in_ / groups, 3, 3], seed);
            let gamma = sample(&[out], seed + 1);
            let beta = sample(&[out], seed + 2);
            let mean = sample(&[out], seed + 3);
            let var = (sample(&[out], seed + 4).abs().unwrap() + 0.1).unwrap();
            let named = |n: &str, t: &Tensor| (n.to_owned(), t.clone());
            let vb = VarBuilder::from_tensors(
                [
                    named("c.weight", &w),
                    named("bn.weight", &gamma),
                    named("bn.bias", &beta),
                    named("bn.running_mean", &mean),
                    named("bn.running_var", &var),
                ]
                .into(),
                DType::F32,
                &Device::Cpu,
            );
            let cfg = Conv2dConfig {
                padding: 1,
                stride,
                groups,
                ..Default::default()
            };
            let folded = super::super::tiny_vit::Conv2dBN::new(in_, out, 3, cfg, vb).unwrap();
            let x = sample(&[2, in_, 9, 11], seed + 5);
            let bn = BatchNorm::new(out, mean, var, gamma, beta, 1e-5).unwrap();
            let theirs = x
                .conv2d(&w, 1, stride, 1, groups)
                .unwrap()
                .apply_t(&bn, false)
                .unwrap();
            let diff = max_diff(&folded.forward(&x).unwrap(), &theirs);
            assert!(
                diff < 1e-4,
                "{in_}->{out} / {groups}, stride {stride}: {diff}"
            );
        }
    }
}

/// The encoder's parallel GELU matches candle's (tanh approximation), on a non-contiguous input.
#[test]
fn parallel_gelu_matches_candles() {
    use super::super::tiny_vit::GeluExt;
    let x = (sample(&[3, 40, 50], 7) * 6.0)
        .unwrap()
        .transpose(1, 2)
        .unwrap();
    assert!(max_diff(&x.gelu_par().unwrap(), &x.gelu().unwrap()) < 1e-5);
}

/// The vendored encoder's depthwise kernel matches candle's grouped convolution.
#[test]
fn depthwise_matches_grouped_conv() {
    let x = sample(&[2, 6, 9, 11], 1);
    let w = sample(&[6, 1, 3, 3], 2);
    let bias = sample(&[6], 3);
    for stride in [1, 2] {
        let ours = super::super::tiny_vit::depthwise(&x, &w, &bias, stride, 1).unwrap();
        let theirs = x
            .conv2d(&w, 1, stride, 1, 6)
            .unwrap()
            .broadcast_add(&bias.reshape((1, 6, 1, 1)).unwrap())
            .unwrap();
        assert!(max_diff(&ours, &theirs) < 1e-4);
    }
}

/// The decoder's matrix-product upscaling matches candle's transposed convolution.
#[test]
fn upscale_matches_transposed_conv() {
    let x = sample(&[2, 8, 5, 7], 4);
    let w = sample(&[8, 4, 2, 2], 5);
    let bias = sample(&[4], 6);
    let up = super::super::mask_decoder::Up2::new(&w, &bias).unwrap();
    let ours = up.forward(&x).unwrap();
    let theirs = x
        .conv_transpose2d(&w, 0, 0, 2, 1)
        .unwrap()
        .broadcast_add(&bias.reshape((1, 4, 1, 1)).unwrap())
        .unwrap();
    assert_eq!(ours.dims(), [2, 4, 10, 14]);
    assert!(max_diff(&ours, &theirs) < 1e-4);
}

/// End to end on the bundled weights: the vendored encoder and decoder against candle's originals
/// (`cargo test --release -p pdn-core --features sam --lib vendored_model -- --ignored`; about
/// 7 s).
#[test]
#[ignore = "slow outside release builds"]
fn vendored_model_matches_candles() {
    use candle_transformers::models::segment_anything as sa;
    let vb = VarBuilder::from_slice_safetensors(WEIGHTS, DType::F32, &Device::Cpu).unwrap();
    let x = sample(&[1, 3, 1024, 1024], 11);
    let ours = super::super::tiny_vit::tiny_vit_5m(vb.pp("image_encoder"))
        .unwrap()
        .forward(&x, &Interrupt::NONE)
        .unwrap();
    let theirs = sa::tiny_vit::tiny_vit_5m(vb.pp("image_encoder"))
        .unwrap()
        .forward(&x)
        .unwrap();
    let diff = max_diff(&ours, &theirs);
    assert!(diff < 1e-4, "encoder {diff}");
    let pv = vb.pp("prompt_encoder");
    let prompt =
        sa::prompt_encoder::PromptEncoder::new(256, (64, 64), (1024, 1024), 16, pv).unwrap();
    let points = Tensor::new(&[[[500f32, 400.0]]], &Device::Cpu).unwrap();
    let labels = Tensor::new(&[[1f32]], &Device::Cpu).unwrap();
    let (sparse, dense) = prompt
        .forward(Some((&points, &labels)), None, None)
        .unwrap();
    let pe = prompt.get_dense_pe().unwrap();
    let mv = vb.pp("mask_decoder");
    let (m1, i1) = super::super::mask_decoder::MaskDecoder::new(256, 3, 3, 256, mv.clone())
        .unwrap()
        .forward(&ours, &pe, &sparse, &dense, true)
        .unwrap();
    let (m2, i2) = sa::mask_decoder::MaskDecoder::new(256, 3, 3, 256, mv)
        .unwrap()
        .forward(&ours, &pe, &sparse, &dense, true)
        .unwrap();
    let (dm, di) = (max_diff(&m1, &m2), max_diff(&i1, &i2));
    assert!(dm < 1e-3 && di < 1e-4, "decoder masks {dm}, IoU {di}");
}

/// A 2048×1024 canvas at (100, 50), half size in the frame.
fn wide() -> Embedding {
    Embedding {
        tensor: Tensor::zeros(1, DType::F32, &Device::Cpu).unwrap(),
        canvas: Rect::new(100, 50, 2148, 1074),
        scale: 0.5,
        size: (1024, 512),
    }
}

/// A box prompt is its two corners, in frame coordinates normalised at pixel centres, with no
/// padding token; a point alone gets one.
#[test]
fn prompt_points_map_into_the_frame() {
    let e = wide();
    let points = box_points(e.canvas);
    assert_eq!(points.map(|p| p.2), [TOP_LEFT, BOTTOM_RIGHT]);
    let n = |v: f32| (v + 0.5) / FRAME as f32 * 2.0 - 1.0;
    assert_eq!(
        prompt_coords(&e, &points),
        [n(0.0), n(0.0), n(1024.0), n(512.0)]
    );
    assert_eq!(
        prompt_coords(&e, &[(612.0, 306.0, POINT)]),
        [n(256.0), n(128.0)]
    );
    assert!(!padded(&points));
    assert!(padded(&[(612.0, 306.0, POINT)]));
}

/// Select Subject's prompt: a point at the salient square's centre and a box around it a
/// hundredth of the canvas wider; a flat canvas has no subject.
#[test]
fn subject_prompt_boxes_the_salient_blob() {
    let c = Rect::new(0, 0, 300, 200);
    let img = image::RgbaImage::from_fn(300, 200, |x, y| {
        let on = (100..180).contains(&x) && (60..140).contains(&y);
        image::Rgba(if on {
            [200, 30, 30, 255]
        } else {
            [240, 240, 235, 255]
        })
    });
    let [(px, py, kind), tl, br] = subject_prompt(c, &Picture(img)).unwrap();
    assert_eq!((kind, tl.2, br.2), (POINT, TOP_LEFT, BOTTOM_RIGHT));
    assert!(
        (px - 140.0).abs() < 6.0 && (py - 100.0).abs() < 6.0,
        "{px} {py}"
    );
    assert!(tl.0 <= 100.0 && tl.1 <= 60.0 && br.0 >= 180.0 && br.1 >= 140.0);
    assert!(tl.0 >= 90.0 && tl.1 >= 50.0 && br.0 <= 190.0 && br.1 <= 150.0);
    let flat = image::RgbaImage::from_pixel(300, 200, image::Rgba([128, 128, 128, 255]));
    assert!(subject_prompt(c, &Picture(flat)).is_none());
}

/// Select Subject drops a box answer that is the backdrop: half the border, or three sides.
#[test]
fn backdrop_rule() {
    let e = wide(); // 256 × 128 cells
    let mask = |f: &dyn Fn(usize, usize) -> bool| -> Vec<f32> {
        (0..LOW * LOW)
            .map(|i| if f(i % LOW, i / LOW) { 1.0 } else { -1.0 })
            .collect()
    };
    let middle = mask(&|x, y| (100..150).contains(&x) && (40..90).contains(&y));
    let corner = mask(&|x, y| x < 40 && y < 40);
    assert!(!backdrop(&middle, &e) && !backdrop(&corner, &e));
    let three_sides =
        mask(&|x, y| (x < 3 && (20..110).contains(&y)) || (x < 40 && !(3..125).contains(&y)));
    assert!(!is_background(&three_sides, &e) && backdrop(&three_sides, &e));
    assert!(backdrop(&vec![1.0; LOW * LOW], &e));
}

/// The bundled weights are the pinned file: an accidental change to
/// `assets/models/mobilesam/mobile_sam-tiny-vitt.safetensors` fails here.
#[test]
fn bundled_weights_are_the_pinned_file() {
    use sha2::Digest;
    assert_eq!(WEIGHTS.len() as u64, SIZE);
    let hex: String = sha2::Sha256::digest(WEIGHTS)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(hex, SHA256);
    assert_eq!(
        SHA256,
        "5ad43a841b57cc798e2837e251c15aa9f602868a1780d195459383d117784cf7"
    );
}

/// The bundled weights build the model: every tensor MobileSAM needs is there, in its shape.
#[test]
fn bundled_weights_load() {
    assert!(model().is_some());
}

/// An image decoded by `image`, for the evaluation.
struct Picture(image::RgbaImage);

impl Sampler for Picture {
    fn rgba(&self, r: Rect) -> Vec<[f32; 4]> {
        let mut out = Vec::with_capacity(r.width() as usize * r.height() as usize);
        for y in r.y0..r.y1 {
            for x in r.x0..r.x1 {
                let p = if x >= 0
                    && y >= 0
                    && (x as u32) < self.0.width()
                    && (y as u32) < self.0.height()
                {
                    self.0
                        .get_pixel(x as u32, y as u32)
                        .0
                        .map(|v| f32::from(v) / 255.0)
                } else {
                    [0.0; 4]
                };
                out.push(p);
            }
        }
        out
    }
}

enum Prompt {
    Rect(Rect),
    Point(i32, i32),
    Subject,
}

/// The picture with everything outside `r` darkened, its edge blue, the prompt red.
fn overlay(img: &image::RgbaImage, r: Option<&Region>, prompt: &Prompt) -> image::RgbImage {
    let (w, h) = img.dimensions();
    let cov = |x: i32, y: i32| r.map_or(0.0, |r| r.at(x, y));
    let mut out = image::RgbImage::from_fn(w, h, |x, y| {
        let p = img.get_pixel(x, y).0;
        let c = cov(x as i32, y as i32);
        let k = 0.3 + 0.7 * c;
        image::Rgb([0, 1, 2].map(|i| (f32::from(p[i]) * k) as u8))
    });
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let on = cov(x, y) >= 0.5;
            if on != (cov(x + 1, y) >= 0.5) || on != (cov(x, y + 1) >= 0.5) {
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    if (x + dx) < w as i32 && (y + dy) < h as i32 {
                        out.put_pixel((x + dx) as u32, (y + dy) as u32, image::Rgb([40, 120, 255]));
                    }
                }
            }
        }
    }
    let red = image::Rgb([255, 40, 40]);
    match *prompt {
        Prompt::Rect(b) => {
            for t in 0..3 {
                for x in b.x0..b.x1 {
                    for y in [b.y0 + t, b.y1 - 1 - t] {
                        if (0..w as i32).contains(&x) && (0..h as i32).contains(&y) {
                            out.put_pixel(x as u32, y as u32, red);
                        }
                    }
                }
                for y in b.y0..b.y1 {
                    for x in [b.x0 + t, b.x1 - 1 - t] {
                        if (0..w as i32).contains(&x) && (0..h as i32).contains(&y) {
                            out.put_pixel(x as u32, y as u32, red);
                        }
                    }
                }
            }
        }
        Prompt::Point(px, py) => {
            for dy in -8..=8 {
                for dx in -8..=8 {
                    let (x, y) = (px + dx, py + dy);
                    if dx * dx + dy * dy <= 64
                        && (0..w as i32).contains(&x)
                        && (0..h as i32).contains(&y)
                    {
                        out.put_pixel(x as u32, y as u32, red);
                    }
                }
            }
        }
        Prompt::Subject => {}
    }
    out
}

fn ms(t: std::time::Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

/// Timings and before/after renders on real pictures (GrabCut | MobileSAM side by side):
/// `PAINTRS_SAM_EVAL=<dir with inputs/> cargo test --release -p pdn-core --features sam --lib
/// sam_eval -- --ignored --nocapture`.
#[test]
#[ignore = "needs the pictures; slow"]
fn sam_eval() {
    let Some(dir) = std::env::var_os("PAINTRS_SAM_EVAL") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(dir.join("renders")).unwrap();
    let t = std::time::Instant::now();
    let model = Model::load(WEIGHTS).unwrap();
    println!("load {:.0} ms", ms(t));
    let cases: &[(&str, &[Prompt])] = &[
        (
            "14-select-teapot",
            &[
                Prompt::Rect(Rect::new(60, 440, 1190, 1500)),
                Prompt::Point(600, 1000),
                Prompt::Subject,
            ],
        ),
        (
            "15-select-helmet",
            &[
                Prompt::Rect(Rect::new(10, 50, 1180, 1340)),
                Prompt::Point(650, 700),
                Prompt::Subject,
            ],
        ),
        (
            "16-select-violin",
            &[
                Prompt::Rect(Rect::new(360, 130, 840, 1470)),
                Prompt::Point(600, 1150),
                Prompt::Point(600, 500),
                Prompt::Subject,
            ],
        ),
        (
            "11-spare-still-life",
            &[
                Prompt::Rect(Rect::new(120, 560, 580, 1010)),
                Prompt::Point(700, 980),
                Prompt::Point(820, 750),
                Prompt::Subject,
            ],
        ),
        (
            "08-liquify-portrait",
            &[Prompt::Point(620, 330), Prompt::Subject],
        ),
    ];
    for (name, prompts) in cases {
        let img = image::open(dir.join("inputs").join(format!("{name}.jpg")))
            .unwrap()
            .to_rgba8();
        let canvas = Rect::new(0, 0, img.width() as i32, img.height() as i32);
        let pic = Picture(img);
        // The first run warms the allocator and thread pool; the second is the one reported.
        let _ = model.embed(&pic, canvas, &Interrupt::NONE).unwrap();
        let t = std::time::Instant::now();
        let e = model.embed(&pic, canvas, &Interrupt::NONE).unwrap();
        println!(
            "{name} {}x{}: encoder {:.0} ms",
            canvas.width(),
            canvas.height(),
            ms(t)
        );
        for (i, p) in prompts.iter().enumerate() {
            let t = std::time::Instant::now();
            let gc = match *p {
                Prompt::Rect(r) => grabcut::object_in_rect(&pic, r, canvas, 160_000),
                Prompt::Point(x, y) => {
                    grabcut::object_at(&pic, canvas, x as f32, y as f32, grabcut::OBJECT_AT_WORK_PX)
                }
                Prompt::Subject => {
                    subject::select_subject(&pic, canvas, subject::WORK_PX, &Interrupt::NONE)
                        .unwrap()
                }
            };
            let gc_ms = ms(t);
            let t = std::time::Instant::now();
            let sam = match *p {
                Prompt::Rect(r) => model.object_in_rect(&e, r),
                Prompt::Point(x, y) => model.object_at(&e, x as f32, y as f32),
                Prompt::Subject => model.subject(&e, &pic),
            }
            .unwrap();
            let sam_ms = ms(t);
            let kind = match p {
                Prompt::Rect(_) => "box",
                Prompt::Point(..) => "point",
                Prompt::Subject => "subject",
            };
            println!("  {i} {kind}: grabcut {gc_ms:.0} ms, sam decoder+mask {sam_ms:.0} ms");
            let (a, b) = (
                overlay(&pic.0, gc.as_ref(), p),
                overlay(&pic.0, sam.as_ref(), p),
            );
            let (w, h) = a.dimensions();
            let mut pair = image::RgbImage::new(w * 2 + 16, h);
            image::imageops::replace(&mut pair, &a, 0, 0);
            image::imageops::replace(&mut pair, &b, i64::from(w) + 16, 0);
            let pair = image::imageops::resize(
                &pair,
                pair.width() / 2,
                h / 2,
                image::imageops::FilterType::Triangle,
            );
            pair.save(dir.join("renders").join(format!("{name}-{i}-{kind}.jpg")))
                .unwrap();
        }
    }
}
