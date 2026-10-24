//! Tests for NAL splitting, headers and emulation prevention.

use super::{NalHeader, Rbsp, split_annexb};
use crate::hevc::error::Error;

#[test]
fn emulation_prevention_bytes_are_removed() {
    // Header, then 00 00 03 01 -> 00 00 01, and 00 00 03 03 -> 00 00 03.
    let nal = [
        0x26u8, 0x01, 0x00, 0x00, 0x03, 0x01, 0xaa, 0x00, 0x00, 0x03, 0x03,
    ];
    let r = Rbsp::from_nal(&nal).expect("rbsp");
    assert_eq!(r.data, [0x00, 0x00, 0x01, 0xaa, 0x00, 0x00, 0x03]);
    assert_eq!(r.epb, [4, 9]);
}

#[test]
fn offsets_map_back_and_forth_across_removed_bytes() {
    let nal = [0x26u8, 0x01, 0x00, 0x00, 0x03, 0x01, 0xaa, 0xbb, 0xcc];
    let r = Rbsp::from_nal(&nal).expect("rbsp");
    for rbsp_off in 0..r.data.len() {
        let nal_off = r.rbsp_to_nal(rbsp_off);
        assert_eq!(r.nal_to_rbsp(nal_off), rbsp_off, "round trip at {rbsp_off}");
    }
}

#[test]
fn a_payload_without_escapes_is_copied_verbatim() {
    let nal = [0x40u8, 0x01, 1, 2, 3, 4, 5];
    let r = Rbsp::from_nal(&nal).expect("rbsp");
    assert_eq!(r.data, [1, 2, 3, 4, 5]);
    assert!(r.epb.is_empty());
}

#[test]
fn nal_headers_decompose_correctly() {
    let h = NalHeader::parse(&[0x26, 0x01]).expect("header");
    assert_eq!(h.nal_unit_type, 19);
    assert_eq!(h.layer_id, 0);
    assert_eq!(h.temporal_id, 0);
    assert!(h.is_vcl() && h.is_irap());
    let h = NalHeader::parse(&[0x42, 0x01]).expect("header");
    assert_eq!(h.nal_unit_type, 33);
    assert!(!h.is_vcl());
    // nuh_layer_id spans the low bit of byte 0 and the high five of byte 1.
    let h = NalHeader::parse(&[0x41, 0x09]).expect("header");
    assert_eq!(h.layer_id, 33);
    assert_eq!(h.temporal_id, 0);
    assert_eq!(
        NalHeader::parse(&[0x80, 0x01]),
        Err(Error::InvalidData("forbidden_zero_bit set"))
    );
    assert_eq!(NalHeader::parse(&[0x26]), Err(Error::Truncated));
}

#[test]
fn annex_b_splitting_handles_both_start_code_lengths() {
    let stream = [
        0u8, 0, 0, 1, 0x40, 0x01, 0xaa, // four byte start code
        0, 0, 1, 0x42, 0x01, 0xbb, 0xcc, // three byte start code
        0, 0, 0, 1, 0x44, 0x01, 0xdd,
    ];
    let units = split_annexb(&stream);
    assert_eq!(units.len(), 3);
    assert_eq!(units[0], &[0x40, 0x01, 0xaa]);
    assert_eq!(units[1], &[0x42, 0x01, 0xbb, 0xcc]);
    assert_eq!(units[2], &[0x44, 0x01, 0xdd]);
    assert!(split_annexb(&[1, 2, 3]).is_empty());
}
