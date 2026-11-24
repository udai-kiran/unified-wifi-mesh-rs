// SPDX-License-Identifier: Apache-2.0

//! Seeded model test of the `Reassembler` against a plain reference model, and the properties
//! P0, P2', P3 and P5 of 0016 D7 (0017 A3). The reference model may use iterators and sums;
//! the real code may not.

use super::*;

/// The local address used by the helpers below that do not vary it.
const LOCAL: MacAddr = MacAddr::new([0x02, 0, 0, 0, 0, 0x0a]);

/// Local addresses and remote addresses the model test picks from; a "peer" is one pair.
const LOCAL_COUNT: usize = 2;
const REMOTE_COUNT: usize = 3;

/// xorshift64: a tiny deterministic generator, so no dependency is needed.
struct Prng(u64);

impl Prng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// A number in `0..limit`; `limit` must be above 0.
    fn below(&mut self, limit: usize) -> usize {
        let limit = u64::try_from(limit).unwrap();
        usize::try_from(self.next() % limit).unwrap()
    }
}

fn local_mac(index: usize) -> MacAddr {
    MacAddr::new([0x02, 0, 0, 0, 0, u8::try_from(0x20 + index).unwrap()])
}

fn peer(index: usize) -> MacAddr {
    MacAddr::new([0x02, 0, 0, 0, 0, u8::try_from(0x10 + index).unwrap()])
}

fn frame_from(remote: MacAddr, flags: (u8, u8), id: u8, payload: Vec<u8>) -> SduFrame {
    frame_between(LOCAL, remote, flags, id, payload)
}

fn frame_between(
    local: MacAddr,
    remote: MacAddr,
    flags: (u8, u8),
    id: u8,
    payload: Vec<u8>,
) -> SduFrame {
    SduFrame {
        local,
        remote,
        is_fragment: flags.0,
        is_last_fragment: flags.1,
        fragment_id: id,
        payload,
    }
}

/// What the model says one frame does.
#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    delivered: Option<Sdu>,
    dropped: u64,
    too_large: bool,
}

enum ModelState {
    Idle,
    /// A set being joined: its addresses, the next id, and the payloads joined so far.
    Open {
        local: MacAddr,
        remote: MacAddr,
        next_id: u16,
        parts: Vec<Vec<u8>>,
    },
    /// A refused set: its addresses and the next id of its frames.
    Skipping {
        local: MacAddr,
        remote: MacAddr,
        next_id: u16,
    },
}

struct Model {
    cap: usize,
    state: ModelState,
}

impl Model {
    fn push(&mut self, frame: &SduFrame) -> Outcome {
        let is_whole = frame.is_fragment == 0 && frame.is_last_fragment == 1;
        let is_last = frame.is_last_fragment == 1;
        let id = u16::from(frame.fragment_id);
        let nothing = |dropped| Outcome {
            delivered: None,
            dropped,
            too_large: false,
        };

        if is_whole {
            let dropped = u64::from(matches!(self.state, ModelState::Open { .. }));
            self.state = ModelState::Idle;
            return Outcome {
                delivered: Some(Sdu {
                    local: frame.local,
                    remote: frame.remote,
                    cmdu: Vec::from(frame.payload.as_slice()),
                }),
                dropped,
                too_large: false,
            };
        }

        // An id 0 frame always starts a set; what it replaces is counted if it was open.
        let mut dropped = 0;
        if id == 0 {
            dropped = u64::from(matches!(self.state, ModelState::Open { .. }));
            self.state = ModelState::Open {
                local: frame.local,
                remote: frame.remote,
                next_id: 0,
                parts: Vec::new(),
            };
        }

        match &mut self.state {
            ModelState::Idle => nothing(dropped + 1),
            ModelState::Skipping {
                local,
                remote,
                next_id,
            } => {
                if id == *next_id && frame.local == *local && frame.remote == *remote {
                    *next_id += 1;
                    if is_last {
                        self.state = ModelState::Idle;
                    }
                    nothing(0)
                } else {
                    self.state = ModelState::Idle;
                    nothing(1)
                }
            }
            ModelState::Open {
                local,
                remote,
                next_id,
                parts,
            } => {
                if id != *next_id || frame.local != *local || frame.remote != *remote {
                    self.state = ModelState::Idle;
                    return nothing(dropped + 2);
                }
                let joined: usize = parts.iter().map(Vec::len).sum();
                if joined + frame.payload.len() > self.cap {
                    self.state = if is_last {
                        ModelState::Idle
                    } else {
                        ModelState::Skipping {
                            local: *local,
                            remote: *remote,
                            next_id: *next_id + 1,
                        }
                    };
                    return Outcome {
                        delivered: None,
                        dropped,
                        too_large: true,
                    };
                }
                parts.push(Vec::from(frame.payload.as_slice()));
                *next_id += 1;
                if !is_last {
                    return nothing(dropped);
                }
                let delivered = Sdu {
                    local: *local,
                    remote: *remote,
                    cmdu: parts.concat(),
                };
                self.state = ModelState::Idle;
                Outcome {
                    delivered: Some(delivered),
                    dropped,
                    too_large: false,
                }
            }
        }
    }

    /// What `abandon_set` should return.
    fn abandon(&mut self) -> u64 {
        let open = matches!(self.state, ModelState::Open { .. });
        self.state = ModelState::Idle;
        u64::from(open)
    }
}

/// A random frame. Peers are every pair of a local and a remote address. Per peer, `next_ids`
/// holds the id that continues that peer's last set, so that sets often finish and refusals
/// happen; other ids and flags are random. Payload octets count up from `next_byte`, so that
/// a wrong order or a wrong join shows in the delivered bytes.
fn random_frame(rng: &mut Prng, next_ids: &mut [u8], next_byte: &mut u8) -> SduFrame {
    let peer_index = rng.below(next_ids.len());
    let local = local_mac(peer_index % LOCAL_COUNT);
    let remote = peer(peer_index / LOCAL_COUNT);
    let mut payload = Vec::new();
    for _ in 0..rng.below(5) {
        payload.push(*next_byte);
        *next_byte = next_byte.wrapping_add(1);
    }
    let kind = rng.below(10);
    if kind == 0 {
        // whole, with any id
        let id = u8::try_from(rng.below(4)).unwrap();
        next_ids[peer_index] = 0;
        return frame_between(local, remote, (0, 1), id, payload);
    }
    let id = match kind {
        1 => 0,
        2 => u8::try_from(rng.below(4)).unwrap(),
        _ => next_ids[peer_index],
    };
    let is_last = u8::from(rng.below(4) == 0);
    let is_fragment = u8::from(rng.below(8) != 0);
    next_ids[peer_index] = if is_last == 1 { 0 } else { id.wrapping_add(1) };
    frame_between(local, remote, (is_fragment, is_last), id, payload)
}

fn describe(frame: &SduFrame) -> String {
    format!(
        "local {:?} remote {:?} flags ({}, {}) id {} payload {:?}",
        frame.local,
        frame.remote,
        frame.is_fragment,
        frame.is_last_fragment,
        frame.fragment_id,
        frame.payload
    )
}

#[test]
fn model_test_reassembler_matches_the_reference_model() {
    let peer_count = LOCAL_COUNT * REMOTE_COUNT;
    let mut refusals_seen = 0_u64;
    let mut skips_seen = 0_u64;
    for seed in 1..=300_u64 {
        let mut rng = Prng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15));
        let cap = 1 + rng.below(8);
        let mut real = Reassembler::new(cap);
        let mut model = Model {
            cap,
            state: ModelState::Idle,
        };
        let mut next_ids = vec![0_u8; peer_count];
        let mut next_byte = 0_u8;
        let mut history: Vec<String> = Vec::new();
        for _ in 0..200 {
            if rng.below(40) == 0 {
                assert_eq!(
                    real.abandon_set(),
                    model.abandon(),
                    "seed {seed} cap {cap}: abandon_set after {history:?}"
                );
                next_ids.fill(0);
                history.push("abandon_set".to_owned());
                continue;
            }
            let frame = random_frame(&mut rng, &mut next_ids, &mut next_byte);
            history.push(describe(&frame));
            let was_skipping = matches!(model.state, ModelState::Skipping { .. });
            let was_whole = frame.is_fragment == 0 && frame.is_last_fragment == 1;
            let expected = model.push(&frame);
            let step = real.push(frame);
            let actual = Outcome {
                delivered: step.sdu,
                dropped: step.dropped,
                too_large: step.too_large,
            };
            assert_eq!(actual, expected, "seed {seed} cap {cap}: after {history:?}");
            refusals_seen += u64::from(expected.too_large);
            skips_seen += u64::from(was_skipping && expected.dropped == 0);
            if let Some(delivered) = &expected.delivered {
                assert!(
                    was_whole || delivered.cmdu.len() <= cap,
                    "seed {seed}: a fragmented delivery passed the cap"
                );
            }
        }
    }
    // The generator must reach the cases the test is about.
    assert!(refusals_seen > 100, "only {refusals_seen} refusals");
    assert!(skips_seen > 100, "only {skips_seen} skipped frames");
}

fn pattern(length: usize, seed: u8) -> Vec<u8> {
    (0..length)
        .map(|index| seed.wrapping_add(u8::try_from(index % 251).unwrap()))
        .collect()
}

fn push_all(joiner: &mut Reassembler, sdu: &Sdu) -> (Vec<Sdu>, u64) {
    let mut delivered = Vec::new();
    let mut dropped = 0;
    for frame in encode_sdu_frames(sdu).unwrap() {
        let step = joiner.push(decode_sdu_frame(&frame[LENGTH_PREFIX_LEN..]).unwrap());
        assert!(!step.too_large);
        dropped += step.dropped;
        delivered.extend(step.sdu);
    }
    (delivered, dropped)
}

fn sdu_of(cmdu: Vec<u8>) -> Sdu {
    Sdu {
        local: LOCAL,
        remote: peer(0),
        cmdu,
    }
}

#[test]
fn p0_round_trip_under_the_cap() {
    // 16 fragments is the most that fits the 1 MiB cap; with a cap of the encoder's limit
    // the same property holds up to 256 fragments.
    let sizes = [
        0,
        1,
        MAX_FRAGMENT_PAYLOAD,
        MAX_FRAGMENT_PAYLOAD + 1,
        3 * MAX_FRAGMENT_PAYLOAD + 5,
        16 * MAX_FRAGMENT_PAYLOAD,
    ];
    for size in sizes {
        let original = sdu_of(pattern(size, 7));
        let mut joiner = Reassembler::new(crate::policy::RX_SDU_CAP);
        let (delivered, dropped) = push_all(&mut joiner, &original);
        assert_eq!(delivered, [original], "size {size}");
        assert_eq!(dropped, 0, "size {size}");
    }
    for size in [100 * MAX_FRAGMENT_PAYLOAD + 3, MAX_SDU_PAYLOAD] {
        let original = sdu_of(pattern(size, 9));
        let mut joiner = Reassembler::new(MAX_SDU_PAYLOAD);
        let (delivered, dropped) = push_all(&mut joiner, &original);
        assert_eq!(delivered, [original], "size {size}");
        assert_eq!(dropped, 0, "size {size}");
    }
}

#[test]
fn p2_prime_merged_sets_from_one_peer_deliver_an_sdu_equal_to_neither() {
    // Counter-example, not a wanted behaviour: the frame has no message id (0011 A8, "Limits").
    let first = sdu_of(vec![0xaa; MAX_FRAGMENT_PAYLOAD + 1]);
    let second = sdu_of(vec![0xbb; MAX_FRAGMENT_PAYLOAD + 1]);
    let first_frames = encode_sdu_frames(&first).unwrap();
    let second_frames = encode_sdu_frames(&second).unwrap();
    let mut joiner = Reassembler::new(crate::policy::RX_SDU_CAP);
    let decode = |frame: &Vec<u8>| decode_sdu_frame(&frame[LENGTH_PREFIX_LEN..]).unwrap();
    // A0, B0, A1
    joiner.push(decode(&first_frames[0]));
    joiner.push(decode(&second_frames[0]));
    let delivered = joiner.push(decode(&first_frames[1])).sdu.unwrap();
    assert_eq!(delivered.cmdu.len(), MAX_FRAGMENT_PAYLOAD + 1);
    assert_ne!(delivered, first);
    assert_ne!(delivered, second);
}

#[test]
fn p3_dropped_counts_are_abandoned_1_stray_1_broken_continuation_2() {
    let one =
        |joiner: &mut Reassembler, flags, id| joiner.push(frame_from(peer(0), flags, id, vec![1]));
    // abandoned set: counted by the caller of `abandon_set`, 1
    let mut joiner = Reassembler::new(8);
    assert_eq!(one(&mut joiner, (1, 0), 0).dropped, 0);
    assert_eq!(joiner.abandon_set(), 1);
    assert_eq!(joiner.abandon_set(), 0);
    // stray: no set in progress, 1
    assert_eq!(one(&mut joiner, (1, 1), 3).dropped, 1);
    // broken continuation: the frame and the set, 2
    one(&mut joiner, (1, 0), 0);
    assert_eq!(one(&mut joiner, (1, 0), 5).dropped, 2);
}

#[test]
fn p5_partial_payload_never_passes_the_cap_and_a_refusal_is_counted_once() {
    // intentional-difference:rx-sdu-cap-1mib
    for cap in 1..=8_usize {
        for chunk in 1..=3_usize {
            let mut joiner = Reassembler::new(cap);
            let total_fragments = 10;
            let mut joined_len = 0;
            let mut refusals = 0_u64;
            for id in 0..total_fragments {
                let is_last = u8::from(id == total_fragments - 1);
                let step = joiner.push(frame_from(peer(0), (1, is_last), id, vec![0xcd; chunk]));
                assert_eq!(
                    step.sdu, None,
                    "cap {cap} chunk {chunk}: nothing is delivered"
                );
                assert_eq!(
                    step.dropped, 0,
                    "cap {cap} chunk {chunk}: nothing is counted as dropped"
                );
                if step.too_large {
                    refusals += 1;
                } else if refusals == 0 {
                    joined_len += chunk;
                }
            }
            assert!(
                joined_len <= cap,
                "cap {cap} chunk {chunk}: joined {joined_len}"
            );
            // the set is 10 chunks: it passes the cap exactly when 10 * chunk > cap
            assert_eq!(refusals, u64::from(10 * chunk > cap));
            assert_eq!(joiner.abandon_set(), 0);
        }
    }
}
