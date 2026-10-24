//! Golden vector: a synthetic DC-predicted IDR decoded to an exact picture.

use super::synth::{build_pps, build_slice, build_sps, build_vps, picture, picture_fmt, to_nal};
use alloc::vec::Vec;

#[test]
fn synthetic_dc_picture_decodes_to_mid_grey() {
    let (sets_owned, slice) = picture(64, 64);
    let sets: [&[u8]; 3] = [&sets_owned[0], &sets_owned[1], &sets_owned[2]];
    let info = crate::hevc::probe(&sets).expect("probe");
    assert_eq!((info.width, info.height, info.bit_depth), (64, 64, 8));
    assert_eq!(info.chroma, crate::hevc::ChromaFormat::Yuv420);
    let f = crate::hevc::decode_still(&sets, &[&slice]).expect("decode");
    assert_eq!((f.width, f.height), (64, 64));
    assert_eq!(f.y.len(), 64 * 64);
    assert_eq!(f.cb.len(), 32 * 32);
    // Every neighbour is unavailable, so reference substitution yields 1 << 7
    // and the DC predictor reproduces it across the whole picture.
    assert!(f.y.iter().all(|&v| v == 128), "luma is not uniformly 128");
    assert!(f.cb.iter().all(|&v| v == 128), "Cb is not uniformly 128");
    assert!(f.cr.iter().all(|&v| v == 128), "Cr is not uniformly 128");
}

#[test]
fn annex_b_wrapper_reaches_the_same_picture() {
    let mut stream = Vec::new();
    for (t, rbsp) in [
        (32u8, build_vps()),
        (33, build_sps(64, 64, 1, 8)),
        (34, build_pps()),
        (19, build_slice(1, 1)),
    ] {
        stream.extend_from_slice(&[0, 0, 0, 1]);
        stream.extend_from_slice(&to_nal(t, &rbsp));
    }
    let f = crate::hevc::decode_annexb(&stream).expect("decode");
    assert_eq!((f.width, f.height), (64, 64));
    assert!(f.y.iter().all(|&v| v == 128));
}

#[test]
fn a_larger_synthetic_picture_decodes_to_mid_grey() {
    let (sets_owned, slice) = picture(256, 192);
    let sets: [&[u8]; 3] = [&sets_owned[0], &sets_owned[1], &sets_owned[2]];
    let f = crate::hevc::decode_still(&sets, &[&slice]).expect("decode");
    assert_eq!((f.width, f.height), (256, 192));
    assert!(f.y.iter().all(|&v| v == 128));
    assert!(f.cb.iter().all(|&v| v == 128));
    assert!(f.cr.iter().all(|&v| v == 128));
}

#[test]
fn every_chroma_format_and_bit_depth_decodes_to_mid_grey() {
    use crate::hevc::ChromaFormat::{Monochrome, Yuv420, Yuv422, Yuv444};
    for (idc, want) in [(0u32, Monochrome), (1, Yuv420), (2, Yuv422), (3, Yuv444)] {
        for bit_depth in [8u32, 10] {
            let (sets_owned, slice) = picture_fmt(128, 64, idc, bit_depth);
            let sets: [&[u8]; 3] = [&sets_owned[0], &sets_owned[1], &sets_owned[2]];
            let info = crate::hevc::probe(&sets).expect("probe");
            assert_eq!(info.chroma, want, "idc {idc}");
            assert_eq!(info.bit_depth as u32, bit_depth);
            let f = crate::hevc::decode_still(&sets, &[&slice])
                .unwrap_or_else(|e| panic!("idc {idc} depth {bit_depth}: {e}"));
            let mid = 1u16 << (bit_depth - 1);
            assert_eq!((f.width, f.height), (128, 64));
            assert!(
                f.y.iter().all(|&v| v == mid),
                "luma, idc {idc} depth {bit_depth}"
            );
            let (cw, chh) = match want {
                Monochrome => (0, 0),
                Yuv420 => (64, 32),
                Yuv422 => (64, 64),
                Yuv444 => (128, 64),
            };
            assert_eq!(f.cb.len(), cw * chh, "Cb size, idc {idc}");
            assert_eq!(f.c_stride as usize, cw);
            assert!(
                f.cb.iter().all(|&v| v == mid),
                "Cb, idc {idc} depth {bit_depth}"
            );
            assert!(
                f.cr.iter().all(|&v| v == mid),
                "Cr, idc {idc} depth {bit_depth}"
            );
        }
    }
}

#[test]
fn a_tiled_synthetic_picture_decodes_to_mid_grey() {
    let (sets_owned, slice) = super::synth::picture_tiled();
    let sets: [&[u8]; 3] = [&sets_owned[0], &sets_owned[1], &sets_owned[2]];
    let f = crate::hevc::decode_still(&sets, &[&slice]).expect("decode");
    assert_eq!((f.width, f.height), (256, 256));
    assert!(f.y.iter().all(|&v| v == 128), "luma is not uniformly 128");
    assert!(f.cb.iter().all(|&v| v == 128));
    assert!(f.cr.iter().all(|&v| v == 128));
}

/// Builds a slice segment header with an arbitrary `slice_type` and no data.
fn header_with_slice_type(slice_type: u32) -> Vec<u8> {
    let mut w = crate::hevc::bits::BitWriter::default();
    w.put(1, 1); // first_slice_segment_in_pic_flag
    w.put(0, 1); // no_output_of_prior_pics_flag
    w.ue(0); // slice_pic_parameter_set_id
    w.ue(slice_type);
    w.se(0); // slice_qp_delta
    w.put(1, 1); // alignment_bit_equal_to_one
    w.rbsp_trailing();
    to_nal(19, &w.bytes)
}

#[test]
fn inter_coded_slices_are_rejected() {
    let (sets_owned, _) = picture(64, 64);
    let sets: [&[u8]; 3] = [&sets_owned[0], &sets_owned[1], &sets_owned[2]];
    for (slice_type, name) in [(0u32, "B"), (1, "P")] {
        let slice = header_with_slice_type(slice_type);
        assert_eq!(
            crate::hevc::decode_still(&sets, &[&slice]).err(),
            Some(crate::Error::Unsupported(
                "P and B slices (inter prediction)"
            )),
            "slice_type {slice_type} ({name}) should be rejected"
        );
    }
}

#[test]
fn missing_inputs_are_reported_rather_than_guessed() {
    let (sets_owned, slice) = picture(64, 64);
    let sets: [&[u8]; 3] = [&sets_owned[0], &sets_owned[1], &sets_owned[2]];
    assert_eq!(
        crate::hevc::decode_still(&sets, &[]).err(),
        Some(crate::Error::Malformed(
            "the coded item carries no slice NAL units"
        ))
    );
    // Parameter sets without the PPS the slice names.
    let only_sps: [&[u8]; 2] = [&sets_owned[0], &sets_owned[1]];
    assert_eq!(
        crate::hevc::decode_still(&only_sps, &[&slice]).err(),
        Some(crate::Error::MissingParameterSet("PPS"))
    );
    assert!(crate::hevc::probe(&[]).is_err());
}
