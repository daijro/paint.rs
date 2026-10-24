//! Tests for the RBSP bit reader and the test-only bit writer.

use super::{BitReader, BitWriter};
use crate::hevc::error::Error;

fn xorshift(s: &mut u32) -> u32 {
    let mut x = *s;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *s = x;
    x
}

#[test]
fn exp_golomb_round_trips() {
    let mut w = BitWriter::default();
    let mut rng = 0x1234_5678u32;
    let mut values = Vec::new();
    for _ in 0..5000 {
        let v = xorshift(&mut rng) >> (xorshift(&mut rng) % 24);
        values.push(v);
        w.ue(v);
    }
    w.rbsp_trailing();
    let mut r = BitReader::new(&w.bytes);
    for &v in &values {
        assert_eq!(r.ue().expect("ue"), v);
    }
}

#[test]
fn signed_exp_golomb_round_trips() {
    let mut w = BitWriter::default();
    let mut rng = 0x9e37_79b9u32;
    let mut values = Vec::new();
    for _ in 0..5000 {
        let m = xorshift(&mut rng) >> (xorshift(&mut rng) % 20);
        let v = if xorshift(&mut rng) & 1 == 0 {
            m as i32
        } else {
            -(m as i32)
        };
        values.push(v);
        w.se(v);
    }
    w.rbsp_trailing();
    let mut r = BitReader::new(&w.bytes);
    for &v in &values {
        assert_eq!(r.se().expect("se"), v);
    }
}

#[test]
fn fixed_width_fields_round_trip_across_byte_boundaries() {
    let mut w = BitWriter::default();
    let mut rng = 0x0bad_f00du32;
    let mut fields = Vec::new();
    for _ in 0..3000 {
        let n = 1 + xorshift(&mut rng) % 32;
        let v = if n == 32 {
            xorshift(&mut rng)
        } else {
            xorshift(&mut rng) & ((1 << n) - 1)
        };
        fields.push((v, n));
        w.put(v, n);
    }
    w.rbsp_trailing();
    let mut r = BitReader::new(&w.bytes);
    for &(v, n) in &fields {
        assert_eq!(r.u(n).expect("u(n)"), v, "reading {n} bits");
    }
}

#[test]
fn known_exp_golomb_code_words() {
    // Clause 9.2: 1 -> 0, 010 -> 1, 011 -> 2, 00100 -> 3.
    let data = [0b1010_0110, 0b0100_0000];
    let mut r = BitReader::new(&data);
    assert_eq!(r.ue().expect("ue"), 0);
    assert_eq!(r.ue().expect("ue"), 1);
    assert_eq!(r.ue().expect("ue"), 2);
    assert_eq!(r.ue().expect("ue"), 3);
}

#[test]
fn reading_past_the_end_reports_truncation() {
    let data = [0xffu8];
    let mut r = BitReader::new(&data);
    assert_eq!(r.u(8).expect("u(8)"), 255);
    assert_eq!(r.u1(), Err(Error::Truncated));
    let mut r = BitReader::new(&[0x00u8]);
    assert_eq!(r.ue(), Err(Error::Truncated));
}

#[test]
fn more_rbsp_data_stops_at_the_stop_bit() {
    let mut w = BitWriter::default();
    w.put(0b101, 3);
    w.rbsp_trailing();
    let mut r = BitReader::new(&w.bytes);
    assert!(r.more_rbsp_data());
    r.u(3).expect("u(3)");
    assert!(!r.more_rbsp_data());
    // Trailing zero bytes must not be mistaken for payload.
    let mut bytes = w.bytes.clone();
    bytes.extend_from_slice(&[0, 0, 0]);
    let mut r = BitReader::new(&bytes);
    r.u(3).expect("u(3)");
    assert!(!r.more_rbsp_data());
}

#[test]
fn byte_alignment_moves_to_the_next_boundary() {
    let data = [0xabu8, 0xcd, 0xef];
    let mut r = BitReader::new(&data);
    r.u(3).expect("u(3)");
    assert!(!r.is_aligned());
    r.byte_align();
    assert!(r.is_aligned());
    assert_eq!(r.byte_pos(), 1);
    assert_eq!(r.u(8).expect("u(8)"), 0xcd);
}
