//! Self-checking tests for the CABAC engine.
//!
//! The engine is validated against an arithmetic *encoder* transcribed
//! independently from clause 9.3.4.4 of the specification: encoding a random
//! bin sequence and decoding it back must reproduce the sequence exactly,
//! which exercises the range table, both state transition tables,
//! renormalisation, bypass coding and termination.

use super::Cabac;
use super::ctx::{INIT_VALUES, NUM_CTX};
use super::encoder::Enc;
use super::tables::{RANGE_TAB_LPS, TRANS_IDX_LPS, TRANS_IDX_MPS};
use crate::hevc::error::Error;

#[test]
fn running_past_the_end_is_reported_by_the_next_terminate_bin() {
    let mut dec = Cabac::new(&[0u8; 4], 0, 26).expect("decoder init");
    // Sixty-four bypass bins shift in eight bytes: within the overread slack.
    for _ in 0..64 {
        dec.bypass();
    }
    assert_eq!(dec.terminate(), Ok(0));
    // Two hundred more run well past it; the bins themselves never fail.
    for _ in 0..200 {
        dec.bypass();
    }
    assert_eq!(dec.terminate(), Err(Error::Truncated));
}

/// One bin of a test sequence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Bin {
    Ctx(usize, u32),
    Bypass(u32),
    Term(u32),
}

fn xorshift(state: &mut u32) -> u32 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    x
}

fn round_trip(seed: u32, qp: i32, n: usize) {
    let mut rng = seed;
    let mut bins = Vec::with_capacity(n);
    for _ in 0..n {
        let r = xorshift(&mut rng);
        bins.push(match r % 10 {
            0..=6 => Bin::Ctx((r >> 8) as usize % NUM_CTX, (r >> 4) & 1),
            _ => Bin::Bypass((r >> 4) & 1),
        });
    }
    bins.push(Bin::Term(1));
    let mut enc = Enc::new(qp);
    for b in &bins {
        match *b {
            Bin::Ctx(i, v) => enc.decision(i, v),
            Bin::Bypass(v) => enc.bypass(v),
            Bin::Term(v) => enc.terminate(v),
        }
    }
    let data = enc.finish();
    let mut dec = Cabac::new(&data, 0, qp).expect("decoder init");
    for b in &bins {
        let got = match *b {
            Bin::Ctx(i, _) => Bin::Ctx(i, dec.decision(i)),
            Bin::Bypass(_) => Bin::Bypass(dec.bypass()),
            Bin::Term(_) => Bin::Term(dec.terminate().expect("terminate")),
        };
        assert_eq!(got, *b, "bin mismatch, qp {qp}, seed {seed}");
    }
}

#[test]
fn engine_round_trips_random_bin_sequences() {
    for seed in 1..12u32 {
        for qp in [0, 12, 26, 37, 51] {
            round_trip(seed, qp, 4000);
        }
    }
}

#[test]
fn engine_round_trips_terminating_zero_bins() {
    let mut enc = Enc::new(30);
    for i in 0..64 {
        enc.decision(i % NUM_CTX, (i & 1) as u32);
        enc.terminate(0);
    }
    enc.terminate(1);
    let data = enc.finish();
    let mut dec = Cabac::new(&data, 0, 30).expect("decoder init");
    for i in 0..64 {
        assert_eq!(dec.decision(i % NUM_CTX), (i & 1) as u32);
        assert_eq!(dec.terminate().expect("terminate"), 0);
    }
    assert_eq!(dec.terminate().expect("terminate"), 1);
}

#[test]
fn context_initialisation_matches_the_specification_formula() {
    let mut c = Cabac::new(&[0u8; 4], 0, 26).expect("engine init");
    c.init_contexts(26);
    for (i, &init) in INIT_VALUES.iter().enumerate() {
        let m = (init >> 4) as i32 * 5 - 45;
        let n = ((init & 15) as i32) * 8 - 16;
        let pre = (((m * 26) >> 4) + n).clamp(1, 126);
        let mps = u32::from(pre > 63);
        let state = if mps == 1 { pre - 64 } else { 63 - pre };
        assert_eq!(c.ctx[i] >> 1, state as u8, "state for context {i}");
        assert_eq!(u32::from(c.ctx[i] & 1), mps, "mps for context {i}");
    }
}

#[test]
fn state_transition_tables_are_self_consistent() {
    // An MPS never lowers the state index and saturates at 62 then 63.
    for s in 0..63usize {
        assert!(TRANS_IDX_MPS[s] as usize >= s);
        assert!(TRANS_IDX_LPS[s] as usize <= s.max(1));
    }
    assert_eq!(TRANS_IDX_MPS[62], 62);
    assert_eq!(TRANS_IDX_MPS[63], 63);
    assert_eq!(TRANS_IDX_LPS[63], 63);
    // rangeTabLps decreases with the state index within a column.
    for pair in RANGE_TAB_LPS[..63].windows(2) {
        for (hi, lo) in pair[0].iter().zip(pair[1].iter()) {
            assert!(lo <= hi, "rangeTabLps rises from {hi} to {lo}");
        }
    }
}
