//! paint.rs R52 regressions. Uses the existing heic-blocks fixture.
use super::*;
use crate::boxes::BoxHeader;
use crate::hevc::synth;
use crate::props::colr::{MatrixCoefficients, Range};
use crate::props::simple::{Mirror, parse_imir};

// Retain the synthetic SPS up to its last two flags, then supply VUI Annex E.1.
fn sps_with_signal(signal: Option<(bool, Option<(u8, u8, u8)>)>) -> Vec<u8> {
    let rbsp = synth::build_sps(64, 64, 1, 8);
    let mut bits: Vec<bool> = rbsp
        .iter()
        .flat_map(|b| (0..8).rev().map(move |i| b & (1 << i) != 0))
        .collect();
    let stop = bits.iter().rposition(|b| *b).unwrap();
    bits.truncate(stop - 2); // vui_parameters_present and sps_extension_present
    bits.push(signal.is_some());
    if let Some((full, colour)) = signal {
        bits.extend([
            false,
            false,
            true,
            false,
            true,
            false,
            full,
            colour.is_some(),
        ]);
        if let Some((cp, tc, mc)) = colour {
            for byte in [cp, tc, mc] {
                bits.extend((0..8).rev().map(|i| byte & (1 << i) != 0));
            }
        }
        bits.extend([false; 8]); // chroma location, three flags, display window, timing, restriction
    }
    bits.extend([false, true]); // no SPS extension, rbsp_stop_one_bit
    let mut out = vec![0; bits.len().div_ceil(8)];
    for (i, bit) in bits.into_iter().enumerate() {
        out[i / 8] |= u8::from(bit) << (7 - i % 8);
    }
    synth::to_nal(33, &out)
}

fn unlabelled(signal: Option<(bool, Option<(u8, u8, u8)>)>) -> Nclx {
    // Reuse the committed container only; supply synthetic VPS/SPS/PPS, without decoding pixels.
    let mut ctx = Context::open(include_bytes!(
        "../../../../../tests/fixtures/heic-blocks.heic"
    ))
    .unwrap();
    let original = ctx.meta.props.boxes.iter().find(|b| b.is(b"hvcC")).unwrap();
    let mut hvcc = original.payload[..22].to_vec();
    hvcc.push(3); // numOfArrays
    for (kind, nal) in [
        (32, synth::to_nal(32, &synth::build_vps())),
        (33, sps_with_signal(signal)),
        (34, synth::to_nal(34, &synth::build_pps())),
    ] {
        hvcc.push(kind);
        hvcc.extend_from_slice(&1u16.to_be_bytes());
        hvcc.extend_from_slice(&(nal.len() as u16).to_be_bytes());
        hvcc.extend_from_slice(&nal);
    }
    for property in &mut ctx.meta.props.boxes {
        if property.is(b"hvcC") {
            property.payload = &hvcc;
        }
    }
    super::unlabelled_nclx(&ctx, ctx.meta.primary).unwrap()
}

#[test]
fn miaf_defaults_apply_without_vui_and_to_unspecified_colour() {
    for signal in [None, Some((true, None)), Some((true, Some((2, 2, 2))))] {
        let n = unlabelled(signal);
        assert_eq!(
            (n.primaries, n.transfer, n.matrix_code, n.matrix, n.range),
            (1, 13, 6, MatrixCoefficients::Bt601, Range::Full)
        );
    }
}

#[test]
fn vui_colour_and_range_reach_unlabelled_container_colour() {
    let n = unlabelled(Some((false, Some((9, 16, 9)))));
    assert_eq!(
        (n.primaries, n.transfer, n.matrix_code, n.matrix, n.range),
        (9, 16, 9, MatrixCoefficients::Bt2020Ncl, Range::Limited)
    );
    let n = unlabelled(Some((false, Some((2, 16, 2)))));
    assert_eq!(
        (n.primaries, n.transfer, n.matrix_code, n.range),
        (1, 16, 6, Range::Limited)
    );
}

#[test]
fn imir_mode_zero_exchanges_top_bottom_and_one_left_right() {
    for (mode, expected) in [
        (0, Mirror::TopBottom),
        (1, Mirror::LeftRight),
        (0xfe, Mirror::TopBottom),
        (0xff, Mirror::LeftRight),
    ] {
        let payload = [mode];
        let b = BoxHeader {
            boxtype: *b"imir",
            payload: &payload,
            uuid: None,
            offset: 0,
        };
        assert_eq!(parse_imir(&b).unwrap(), expected);
    }
    let truncated = BoxHeader {
        boxtype: *b"imir",
        payload: &[],
        uuid: None,
        offset: 0,
    };
    assert!(parse_imir(&truncated).is_err());
}
