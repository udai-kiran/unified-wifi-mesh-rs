// SPDX-License-Identifier: Apache-2.0

//! Wire formats of the AL-SAP protocol: registration, SDU header, fragmentation.
//!
//! Everything here is pure and synchronous. Both sockets carry frames made of a
//! 4-octet big-endian length and that many octets (upstream
//! `al_service_utils.cpp:68-82`; `ieee1905-rs` `al_sap.rs:179-181`).

use em_types::MacAddr;

use crate::{Role, Sdu};

/// Octets of the length prefix of a frame.
pub(crate) const LENGTH_PREFIX_LEN: usize = 4;

/// Octets of an SDU header inside a frame: two MAC addresses and three flag octets
/// (upstream `al_service_data_unit.cpp:40-97`, `ieee1905-rs` `sdu_codec.rs:67-151`).
pub(crate) const SDU_HEADER_LEN: usize = 15;

/// Largest body length, the value of the length prefix, accepted on receive. A longer
/// frame is skipped, not read into memory (0011 D3 step 6). Upstream throws above
/// `SOCKET_MTU - 4` octets, 65 532, before it reads the body
/// (`al_service_access_point.cpp:235-237`), so its stream is out of step afterwards.
pub(crate) const MAX_FRAME_LEN: u32 = 65_536;

/// Payload octets in one frame on send: 65 536 less the 4-octet prefix and the
/// 15-octet header (upstream `al_service_access_point.cpp:138-146`,
/// `ieee1905-rs` `al_sap.rs:374-379`).
pub const MAX_FRAGMENT_PAYLOAD: usize = 65_517;

/// Fragment ids are one octet, so one SDU has at most this many fragments.
const MAX_FRAGMENTS: usize = 256;

/// Largest CMDU that can be sent: 256 fragments of [`MAX_FRAGMENT_PAYLOAD`].
pub const MAX_SDU_PAYLOAD: usize = MAX_FRAGMENTS * MAX_FRAGMENT_PAYLOAD;

/// Registration `service_operation`: enable. The client never sends disable.
const SERVICE_OPERATION_ENABLE: u8 = 0x01;

/// Registration `result`: success (`al_service_registration_enums.h`).
pub(crate) const RESULT_SUCCESS: u8 = 0x01;

/// Octets of a registration response body: AL MAC and result.
const REGISTRATION_RESPONSE_LEN: usize = 7;

/// Prepends the length prefix to `body`.
///
/// # Panics
///
/// Never in practice: every body built here is below 4 GiB. The length is
/// checked by `u32::try_from`, and a failure would be a bug in this crate.
fn framed(body: &[u8]) -> Vec<u8> {
    let length = u32::try_from(body.len()).expect("frame bodies are far below 4 GiB");
    let mut frame = Vec::with_capacity(LENGTH_PREFIX_LEN + body.len());
    frame.extend_from_slice(&length.to_be_bytes());
    frame.extend_from_slice(body);
    frame
}

/// The registration request frame (upstream `al_service_registration_request.cpp:35-47`,
/// `ieee1905-rs` `registration_codec.rs:29-93`).
pub(crate) fn encode_registration_request(role: Role) -> Vec<u8> {
    let service_type = match role {
        Role::Agent => 0x01,
        Role::Controller => 0x02,
    };
    framed(&[SERVICE_OPERATION_ENABLE, service_type])
}

/// A complete registration response.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct RegistrationResponse {
    pub(crate) al_mac: MacAddr,
    pub(crate) result: u8,
}

/// Reads a registration response body (upstream `al_service_registration_response.cpp:31-57`).
///
/// Returns `None` for a body shorter than 7 octets. Extra octets are ignored.
pub(crate) fn decode_registration_response(body: &[u8]) -> Option<RegistrationResponse> {
    if body.len() < REGISTRATION_RESPONSE_LEN {
        return None;
    }
    let al_mac = MacAddr::try_from(&body[..6]).ok()?;
    Some(RegistrationResponse {
        al_mac,
        result: body[6],
    })
}

/// A decoded SDU frame: one fragment, or a whole unfragmented CMDU.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct SduFrame {
    pub(crate) local: MacAddr,
    pub(crate) remote: MacAddr,
    pub(crate) is_fragment: u8,
    pub(crate) is_last_fragment: u8,
    pub(crate) fragment_id: u8,
    pub(crate) payload: Vec<u8>,
}

impl SduFrame {
    /// True for a frame that carries a whole CMDU. Upstream tests exactly
    /// `is_fragment == 0 && is_last_fragment == 1`
    /// (`al_service_access_point.cpp:256`, `ieee1905-rs` `al_sap.rs:550`).
    fn is_unfragmented(&self) -> bool {
        self.is_fragment == 0 && self.is_last_fragment == 1
    }
}

/// Reads the SDU header of a frame body. `None` if the body is shorter than the header.
pub(crate) fn decode_sdu_frame(body: &[u8]) -> Option<SduFrame> {
    if body.len() < SDU_HEADER_LEN {
        return None;
    }
    Some(SduFrame {
        local: MacAddr::try_from(&body[0..6]).ok()?,
        remote: MacAddr::try_from(&body[6..12]).ok()?,
        is_fragment: body[12],
        is_last_fragment: body[13],
        fragment_id: body[14],
        payload: body[SDU_HEADER_LEN..].to_vec(),
    })
}

/// The SDU does not fit in 256 fragments.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct SduTooLarge;

/// Encodes `sdu` as complete frames: one, or numbered fragments of at most
/// [`MAX_FRAGMENT_PAYLOAD`] octets (upstream `al_service_access_point.cpp:138-196`).
///
/// MAC A is the local AL MAC and MAC B the remote one (0011, "Address fields").
/// Upstream casts the fragment number to one octet without a check; here a CMDU
/// that would need more than 256 fragments is refused.
pub(crate) fn encode_sdu_frames(sdu: &Sdu) -> Result<Vec<Vec<u8>>, SduTooLarge> {
    if sdu.cmdu.len() > MAX_SDU_PAYLOAD {
        return Err(SduTooLarge);
    }
    if sdu.cmdu.len() <= MAX_FRAGMENT_PAYLOAD {
        return Ok(vec![encode_one_frame(sdu, 0, 1, 0, &sdu.cmdu)]);
    }
    let chunks: Vec<&[u8]> = sdu.cmdu.chunks(MAX_FRAGMENT_PAYLOAD).collect();
    let last_index = chunks.len() - 1;
    let mut frames = Vec::with_capacity(chunks.len());
    for (index, chunk) in chunks.into_iter().enumerate() {
        let is_last = u8::from(index == last_index);
        let fragment_id = u8::try_from(index).map_err(|_| SduTooLarge)?;
        frames.push(encode_one_frame(sdu, 1, is_last, fragment_id, chunk));
    }
    Ok(frames)
}

fn encode_one_frame(
    sdu: &Sdu,
    is_fragment: u8,
    is_last_fragment: u8,
    fragment_id: u8,
    payload: &[u8],
) -> Vec<u8> {
    let mut body = Vec::with_capacity(SDU_HEADER_LEN + payload.len());
    body.extend_from_slice(&sdu.local.octets());
    body.extend_from_slice(&sdu.remote.octets());
    body.push(is_fragment);
    body.push(is_last_fragment);
    body.push(fragment_id);
    body.extend_from_slice(payload);
    framed(&body)
}

/// What one received SDU frame did to the reassembly state.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ReassemblyStep {
    /// A finished SDU, if this frame completed one.
    pub(crate) sdu: Option<Sdu>,
    /// Frames and incomplete sets thrown away because of this frame.
    pub(crate) dropped: u64,
    /// True if this frame made the set in progress pass the size cap, so the set was refused.
    /// Counted apart from `dropped` (0016 D4 "size cap").
    pub(crate) too_large: bool,
}

/// A fragment set being joined.
#[derive(Debug)]
struct PartialSdu {
    local: MacAddr,
    remote: MacAddr,
    /// The fragment id the next frame must carry. A `u16` so that it can pass 255.
    next_id: u16,
    payload: Vec<u8>,
}

/// A refused set whose remaining fragments are thrown away without counting. It has the
/// same shape as a set being joined, but holds no payload.
#[derive(Debug)]
struct SkippedSet {
    local: MacAddr,
    remote: MacAddr,
    /// The fragment id the next frame of the set carries.
    next_id: u16,
}

/// Joins fragments into SDUs (0011 D3 step 6).
///
/// Upstream throws on a fragment out of order (`al_service_access_point.cpp:267-268`).
/// Here the bad fragment and the set in progress are dropped, and the session goes
/// on: a frame with fragment id 0, or an unfragmented one, starts a new SDU.
///
/// A continuation fragment (id above 0) must also carry the same `local` and `remote`
/// as the set it joins, or it is dropped like an out-of-order one (0011 A8,
/// `continuation-mac`). Upstream never compares them. The check only separates SDUs
/// from different peers. Two fragmented SDUs from the same peer carry identical
/// addresses, and `A0 B0 A1` still joins B0 with A1: the frame has no message id.
///
/// A fragmented set whose joined payload would pass the cap is refused (0016 D4 "size cap",
/// U2, U6; `rx-sdu-cap-1mib`): upstream has no cap. The cap is a number passed in (U15). A
/// refused set is reported in `ReassemblyStep::too_large` and none of it is delivered. Its
/// remaining fragments are skipped without counting, until its last fragment. A frame with id
/// 0 or an unfragmented one ends the skip and is handled as usual. A continuation that does
/// not match the skipped set ends the skip and is a stray. Unfragmented frames have no cap
/// here: the frame limit bounds them (0016 E5, U33).
#[derive(Debug)]
pub(crate) struct Reassembler {
    /// Largest joined payload of a set, in octets.
    cap: usize,
    current: Option<PartialSdu>,
    skipping: Option<SkippedSet>,
}

impl Reassembler {
    pub(crate) fn new(cap: usize) -> Self {
        Self {
            cap,
            current: None,
            skipping: None,
        }
    }

    pub(crate) fn push(&mut self, frame: SduFrame) -> ReassemblyStep {
        let mut dropped = 0;

        if frame.is_unfragmented() {
            dropped += self.abandon_set();
            let sdu = Sdu {
                local: frame.local,
                remote: frame.remote,
                cmdu: frame.payload,
            };
            return ReassemblyStep {
                sdu: Some(sdu),
                dropped,
                too_large: false,
            };
        }

        if frame.fragment_id == 0 {
            dropped += self.abandon_set();
            self.current = Some(PartialSdu {
                local: frame.local,
                remote: frame.remote,
                next_id: 0,
                payload: Vec::new(),
            });
        }

        if let Some(skipped) = self.skipping.as_mut() {
            let in_skipped_set = u16::from(frame.fragment_id) == skipped.next_id
                && frame.local == skipped.local
                && frame.remote == skipped.remote;
            if in_skipped_set {
                skipped.next_id += 1;
                if frame.is_last_fragment == 1 {
                    self.skipping = None;
                }
                return ReassemblyStep {
                    sdu: None,
                    dropped,
                    too_large: false,
                };
            }
            // Not part of the refused set: the skip ends and the frame is handled below.
            self.skipping = None;
        }

        let Some(partial) = self.current.as_mut() else {
            // A fragment with id above 0 and no set in progress.
            return ReassemblyStep {
                sdu: None,
                dropped: dropped + 1,
                too_large: false,
            };
        };
        let continues_the_set = u16::from(frame.fragment_id) == partial.next_id
            && frame.local == partial.local
            && frame.remote == partial.remote;
        if !continues_the_set {
            self.current = None;
            return ReassemblyStep {
                sdu: None,
                dropped: dropped + 2,
                too_large: false,
            };
        }
        // `partial.payload` is at most the cap, so the subtraction cannot underflow and the
        // sum of two lengths is never formed.
        if frame.payload.len() > self.cap - partial.payload.len() {
            let refused = SkippedSet {
                local: partial.local,
                remote: partial.remote,
                next_id: partial.next_id + 1,
            };
            self.current = None;
            if frame.is_last_fragment != 1 {
                self.skipping = Some(refused);
            }
            return ReassemblyStep {
                sdu: None,
                dropped,
                too_large: true,
            };
        }
        partial.payload.extend_from_slice(&frame.payload);
        partial.next_id += 1;

        if frame.is_last_fragment != 1 {
            return ReassemblyStep {
                sdu: None,
                dropped,
                too_large: false,
            };
        }
        let sdu = self.current.take().map(|done| Sdu {
            local: done.local,
            remote: done.remote,
            cmdu: done.payload,
        });
        ReassemblyStep {
            sdu,
            dropped,
            too_large: false,
        }
    }

    /// Throws away the set in progress, for example when the session ends.
    /// Returns 1 if there was one. A set being skipped was counted when it was refused, so it
    /// is forgotten and returns 0.
    pub(crate) fn abandon_set(&mut self) -> u64 {
        self.skipping = None;
        u64::from(self.current.take().is_some())
    }
}

#[cfg(test)]
mod model_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::RX_SDU_CAP as CAP;

    const LOCAL: MacAddr = MacAddr::new([0x02, 0, 0, 0, 0, 0x0a]);
    const REMOTE: MacAddr = MacAddr::new([0x02, 0, 0, 0, 0, 0x0b]);

    fn sdu(cmdu: Vec<u8>) -> Sdu {
        Sdu {
            local: LOCAL,
            remote: REMOTE,
            cmdu,
        }
    }

    fn fragment(is_fragment: u8, is_last: u8, id: u8, payload: &[u8]) -> SduFrame {
        SduFrame {
            local: LOCAL,
            remote: REMOTE,
            is_fragment,
            is_last_fragment: is_last,
            fragment_id: id,
            payload: payload.to_vec(),
        }
    }

    #[test]
    fn registration_request_bytes() {
        // length 2, enable (0x01), service type agent 0x01 / controller 0x02
        assert_eq!(
            encode_registration_request(Role::Agent),
            [0, 0, 0, 2, 0x01, 0x01]
        );
        assert_eq!(
            encode_registration_request(Role::Controller),
            [0, 0, 0, 2, 0x01, 0x02]
        );
    }

    #[test]
    fn registration_response_decodes_mac_and_result() {
        let body = [0x02, 0, 0, 0, 0, 0x0a, 0x01];
        assert_eq!(
            decode_registration_response(&body),
            Some(RegistrationResponse {
                al_mac: LOCAL,
                result: RESULT_SUCCESS,
            })
        );
        assert_eq!(decode_registration_response(&body[..6]), None);
    }

    #[test]
    fn unfragmented_sdu_bytes() {
        // prefix 15 + 3 = 18, A, B, is_fragment 0, is_last 1, id 0, payload
        let frames = encode_sdu_frames(&sdu(vec![0xde, 0xad, 0xbe])).unwrap();
        assert_eq!(
            frames,
            [vec![
                0, 0, 0, 18, //
                0x02, 0, 0, 0, 0, 0x0a, //
                0x02, 0, 0, 0, 0, 0x0b, //
                0, 1, 0, //
                0xde, 0xad, 0xbe
            ]]
        );
    }

    #[test]
    fn sdu_header_decodes_field_order() {
        let body = [
            0x02, 0, 0, 0, 0, 0x0a, 0x02, 0, 0, 0, 0, 0x0b, 1, 0, 7, 0x11, 0x22,
        ];
        let frame = decode_sdu_frame(&body).unwrap();
        assert_eq!(frame, fragment(1, 0, 7, &[0x11, 0x22]));
        assert_eq!(decode_sdu_frame(&body[..14]), None);
    }

    #[test]
    fn payload_at_the_limit_is_one_frame_and_one_more_is_two() {
        let one = encode_sdu_frames(&sdu(vec![0; MAX_FRAGMENT_PAYLOAD])).unwrap();
        assert_eq!(one.len(), 1);
        // The whole frame is 65 536 octets with its prefix.
        assert_eq!(one[0].len(), 65_536);

        let two = encode_sdu_frames(&sdu(vec![0; MAX_FRAGMENT_PAYLOAD + 1])).unwrap();
        assert_eq!(two.len(), 2);
        // fragment 0: is_fragment 1, is_last 0, id 0; fragment 1: is_last 1, id 1
        assert_eq!(two[0][16..19], [1, 0, 0]);
        assert_eq!(two[1][16..19], [1, 1, 1]);
        assert_eq!(two[0].len(), 65_536);
        assert_eq!(two[1].len(), LENGTH_PREFIX_LEN + SDU_HEADER_LEN + 1);
    }

    #[test]
    fn sdu_needing_more_than_256_fragments_is_refused() {
        let largest = sdu(vec![0; MAX_SDU_PAYLOAD]);
        assert_eq!(encode_sdu_frames(&largest).unwrap().len(), 256);
        let too_big = sdu(vec![0; MAX_SDU_PAYLOAD + 1]);
        assert_eq!(encode_sdu_frames(&too_big), Err(SduTooLarge));
    }

    #[test]
    fn fragments_join_in_order() {
        let mut joiner = Reassembler::new(CAP);
        let first = joiner.push(fragment(1, 0, 0, b"ab"));
        assert_eq!(
            first,
            ReassemblyStep {
                sdu: None,
                dropped: 0,
                too_large: false
            }
        );
        let second = joiner.push(fragment(1, 0, 1, b"cd"));
        assert_eq!(
            second,
            ReassemblyStep {
                sdu: None,
                dropped: 0,
                too_large: false
            }
        );
        let third = joiner.push(fragment(1, 1, 2, b"e"));
        assert_eq!(third.sdu, Some(sdu(b"abcde".to_vec())));
        assert_eq!(third.dropped, 0);
    }

    #[test]
    fn fragment_out_of_order_drops_the_set_and_id_zero_resyncs() {
        let mut joiner = Reassembler::new(CAP);
        joiner.push(fragment(1, 0, 0, b"ab"));
        // id 2 where 1 was expected: the frame and the set are dropped
        let bad = joiner.push(fragment(1, 0, 2, b"xx"));
        assert_eq!(
            bad,
            ReassemblyStep {
                sdu: None,
                dropped: 2,
                too_large: false
            }
        );
        // the tail of the old set has nothing to join
        let stray = joiner.push(fragment(1, 1, 3, b"yy"));
        assert_eq!(
            stray,
            ReassemblyStep {
                sdu: None,
                dropped: 1,
                too_large: false
            }
        );
        // id 0 starts over
        joiner.push(fragment(1, 0, 0, b"12"));
        let done = joiner.push(fragment(1, 1, 1, b"3"));
        assert_eq!(done.sdu, Some(sdu(b"123".to_vec())));
    }

    /// A fragment of `fragment(..)`'s shape with other addresses.
    fn fragment_from(
        local: MacAddr,
        remote: MacAddr,
        is_last: u8,
        id: u8,
        payload: &[u8],
    ) -> SduFrame {
        SduFrame {
            local,
            remote,
            ..fragment(1, is_last, id, payload)
        }
    }

    const OTHER_MAC: MacAddr = MacAddr::new([0x02, 0, 0, 0, 0, 0x0c]);

    #[test]
    fn continuation_with_a_different_local_drops_the_set() {
        // intentional-difference:continuation-mac
        let mut joiner = Reassembler::new(CAP);
        joiner.push(fragment(1, 0, 0, b"ab"));
        let bad = joiner.push(fragment_from(OTHER_MAC, REMOTE, 0, 1, b"cd"));
        assert_eq!(
            bad,
            ReassemblyStep {
                sdu: None,
                dropped: 2,
                too_large: false
            }
        );
        // the set is gone, so the right fragment after it has nothing to join
        let stray = joiner.push(fragment(1, 1, 2, b"ef"));
        assert_eq!(stray.dropped, 1);
        assert_eq!(stray.sdu, None);
    }

    #[test]
    fn continuation_with_a_different_remote_drops_the_set() {
        // intentional-difference:continuation-mac
        let mut joiner = Reassembler::new(CAP);
        joiner.push(fragment(1, 0, 0, b"ab"));
        let bad = joiner.push(fragment_from(LOCAL, OTHER_MAC, 0, 1, b"cd"));
        assert_eq!(
            bad,
            ReassemblyStep {
                sdu: None,
                dropped: 2,
                too_large: false
            }
        );
        // a new set starts at id 0 as usual
        joiner.push(fragment(1, 0, 0, b"12"));
        let done = joiner.push(fragment(1, 1, 1, b"3"));
        assert_eq!(done.sdu, Some(sdu(b"123".to_vec())));
    }

    #[test]
    fn interleaved_peers_a0_b0_a1_deliver_nothing() {
        // intentional-difference:continuation-mac
        // A's fragment 1 carries the expected id and the last flag, but B's set is open.
        let mut joiner = Reassembler::new(CAP);
        let a0 = joiner.push(fragment(1, 0, 0, b"a0"));
        let b0 = joiner.push(fragment_from(LOCAL, OTHER_MAC, 0, 0, b"b0"));
        let a1 = joiner.push(fragment(1, 1, 1, b"a1"));
        assert_eq!(a0.dropped, 0);
        assert_eq!(b0.dropped, 1);
        assert_eq!(
            a1,
            ReassemblyStep {
                sdu: None,
                dropped: 2,
                too_large: false
            }
        );
        assert_eq!(a0.sdu, None);
        assert_eq!(b0.sdu, None);
    }

    #[test]
    fn known_limit_same_address_a0_b0_a1_still_joins_b0_with_a1() {
        // The check cannot tell two SDUs from one peer apart: the frame has no message id
        // (0011 A8, "Limits"). This pins the limit, not a wanted behaviour.
        let mut joiner = Reassembler::new(CAP);
        joiner.push(fragment(1, 0, 0, b"a0"));
        let b0 = joiner.push(fragment(1, 0, 0, b"b0"));
        assert_eq!(b0.dropped, 1);
        let a1 = joiner.push(fragment(1, 1, 1, b"a1"));
        assert_eq!(a1.sdu, Some(sdu(b"b0a1".to_vec())));
        assert_eq!(a1.dropped, 0);
    }

    #[test]
    fn unfragmented_frame_abandons_a_set_in_progress() {
        let mut joiner = Reassembler::new(CAP);
        joiner.push(fragment(1, 0, 0, b"ab"));
        let step = joiner.push(fragment(0, 1, 0, b"whole"));
        assert_eq!(step.sdu, Some(sdu(b"whole".to_vec())));
        assert_eq!(step.dropped, 1);
    }

    #[test]
    fn id_zero_while_a_set_is_open_restarts_it() {
        let mut joiner = Reassembler::new(CAP);
        joiner.push(fragment(1, 0, 0, b"ab"));
        let restarted = joiner.push(fragment(1, 0, 0, b"cd"));
        assert_eq!(
            restarted,
            ReassemblyStep {
                sdu: None,
                dropped: 1,
                too_large: false
            }
        );
        let done = joiner.push(fragment(1, 1, 1, b"e"));
        assert_eq!(done.sdu, Some(sdu(b"cde".to_vec())));
    }

    #[test]
    fn single_fragment_set_completes() {
        let mut joiner = Reassembler::new(CAP);
        let step = joiner.push(fragment(1, 1, 0, b"only"));
        assert_eq!(step.sdu, Some(sdu(b"only".to_vec())));
    }

    #[test]
    fn frame_with_an_empty_payload_decodes() {
        let body = [0x02, 0, 0, 0, 0, 0x0a, 0x02, 0, 0, 0, 0, 0x0b, 0, 1, 0];
        let frame = decode_sdu_frame(&body).unwrap();
        assert_eq!(frame, fragment(0, 1, 0, &[]));
    }

    #[test]
    fn frame_that_is_not_the_last_and_not_a_fragment_starts_a_set() {
        // Only is_fragment 0 with is_last_fragment 1 is a whole CMDU
        // (`al_service_access_point.cpp:250`).
        let mut joiner = Reassembler::new(CAP);
        let first = joiner.push(fragment(0, 0, 0, b"ab"));
        assert_eq!(
            first,
            ReassemblyStep {
                sdu: None,
                dropped: 0,
                too_large: false
            }
        );
        let second = joiner.push(fragment(1, 1, 1, b"cd"));
        assert_eq!(second.sdu, Some(sdu(b"abcd".to_vec())));
    }

    #[test]
    fn fragments_carry_the_payload_in_order() {
        let pattern = || -> Vec<u8> {
            (0..=MAX_FRAGMENT_PAYLOAD)
                .map(|index| u8::try_from(index % 256).unwrap())
                .collect()
        };
        let expected = pattern();
        let frames = encode_sdu_frames(&sdu(pattern())).unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0][19..], expected[..MAX_FRAGMENT_PAYLOAD]);
        assert_eq!(frames[1][19..], expected[MAX_FRAGMENT_PAYLOAD..]);
    }

    #[test]
    fn completed_set_does_not_survive_its_last_fragment() {
        let mut joiner = Reassembler::new(CAP);
        joiner.push(fragment(1, 0, 0, b"ab"));
        let done = joiner.push(fragment(1, 1, 1, b"cd"));
        assert_eq!(done.sdu, Some(sdu(b"abcd".to_vec())));
        // Neither the next id of a finished set nor the one after it joins anything.
        for id in [1, 2] {
            let stray = joiner.push(fragment(1, 1, id, b"zz"));
            assert_eq!(
                stray,
                ReassemblyStep {
                    sdu: None,
                    dropped: 1,
                    too_large: false
                },
                "stray fragment with id {id}"
            );
        }
    }

    #[test]
    fn unfragmented_frame_closes_the_set_it_abandons() {
        let mut joiner = Reassembler::new(CAP);
        joiner.push(fragment(1, 0, 0, b"ab"));
        joiner.push(fragment(0, 1, 0, b"whole"));
        let stray = joiner.push(fragment(1, 1, 1, b"cd"));
        assert_eq!(
            stray,
            ReassemblyStep {
                sdu: None,
                dropped: 1,
                too_large: false
            }
        );
    }

    /// A reassembler with a cap of 4 octets, and fragments of 2 octets for it.
    fn small() -> Reassembler {
        Reassembler::new(4)
    }

    fn step(sdu: Option<Sdu>, dropped: u64, too_large: bool) -> ReassemblyStep {
        ReassemblyStep {
            sdu,
            dropped,
            too_large,
        }
    }

    #[test]
    fn set_at_the_cap_is_delivered() {
        // intentional-difference:rx-sdu-cap-1mib
        let mut joiner = small();
        joiner.push(fragment(1, 0, 0, b"ab"));
        let done = joiner.push(fragment(1, 1, 1, b"cd"));
        assert_eq!(done, step(Some(sdu(b"abcd".to_vec())), 0, false));
    }

    #[test]
    fn fragment_that_would_pass_the_cap_refuses_the_set_once() {
        // intentional-difference:rx-sdu-cap-1mib
        let mut joiner = small();
        joiner.push(fragment(1, 0, 0, b"ab"));
        joiner.push(fragment(1, 0, 1, b"cd"));
        let refused = joiner.push(fragment(1, 0, 2, b"e"));
        assert_eq!(refused, step(None, 0, true));
        // the rest of the set is skipped without counting, up to its last fragment
        assert_eq!(joiner.push(fragment(1, 0, 3, b"fg")), step(None, 0, false));
        assert_eq!(joiner.push(fragment(1, 1, 4, b"h")), step(None, 0, false));
        // the skip is over: a continuation is a stray
        assert_eq!(joiner.push(fragment(1, 1, 5, b"i")), step(None, 1, false));
    }

    #[test]
    fn fragment_zero_alone_over_the_cap_refuses_at_once() {
        // intentional-difference:rx-sdu-cap-1mib
        let mut joiner = small();
        let refused = joiner.push(fragment(1, 0, 0, b"abcde"));
        assert_eq!(refused, step(None, 0, true));
        assert_eq!(joiner.push(fragment(1, 1, 1, b"f")), step(None, 0, false));
    }

    #[test]
    fn refused_last_fragment_leaves_nothing_to_skip() {
        // intentional-difference:rx-sdu-cap-1mib
        let mut joiner = small();
        joiner.push(fragment(1, 0, 0, b"abc"));
        assert_eq!(joiner.push(fragment(1, 1, 1, b"de")), step(None, 0, true));
        // no skip is left, so the next continuation is a stray
        assert_eq!(joiner.push(fragment(1, 1, 2, b"f")), step(None, 1, false));
    }

    #[test]
    fn unfragmented_frame_over_the_cap_is_delivered() {
        // E5: the cap applies to fragmented sets only; the frame limit bounds a whole frame.
        let mut joiner = small();
        let done = joiner.push(fragment(0, 1, 0, b"abcdefgh"));
        assert_eq!(done, step(Some(sdu(b"abcdefgh".to_vec())), 0, false));
    }

    #[test]
    fn id_zero_ends_the_skip_and_starts_a_new_set() {
        // intentional-difference:rx-sdu-cap-1mib
        let mut joiner = small();
        joiner.push(fragment(1, 0, 0, b"abcde"));
        // a new set, not counted as an abandoned one: the refused set was counted already
        assert_eq!(joiner.push(fragment(1, 0, 0, b"12")), step(None, 0, false));
        let done = joiner.push(fragment(1, 1, 1, b"3"));
        assert_eq!(done, step(Some(sdu(b"123".to_vec())), 0, false));
    }

    #[test]
    fn unfragmented_frame_ends_the_skip_and_is_delivered() {
        // intentional-difference:rx-sdu-cap-1mib
        let mut joiner = small();
        joiner.push(fragment(1, 0, 0, b"abcde"));
        let done = joiner.push(fragment(0, 1, 0, b"whole"));
        assert_eq!(done, step(Some(sdu(b"whole".to_vec())), 0, false));
        assert_eq!(joiner.push(fragment(1, 1, 1, b"x")), step(None, 1, false));
    }

    #[test]
    fn continuation_with_the_wrong_id_ends_the_skip_as_a_stray() {
        // intentional-difference:rx-sdu-cap-1mib
        let mut joiner = small();
        joiner.push(fragment(1, 0, 0, b"abcde"));
        assert_eq!(joiner.push(fragment(1, 0, 3, b"x")), step(None, 1, false));
        // the skip is over: the frame that was in order is a stray too
        assert_eq!(joiner.push(fragment(1, 0, 1, b"y")), step(None, 1, false));
    }

    #[test]
    fn continuation_from_another_peer_ends_the_skip_as_a_stray() {
        // intentional-difference:rx-sdu-cap-1mib
        let mut joiner = small();
        joiner.push(fragment(1, 0, 0, b"abcde"));
        let other = fragment_from(OTHER_MAC, REMOTE, 0, 1, b"x");
        assert_eq!(joiner.push(other), step(None, 1, false));
        assert_eq!(joiner.push(fragment(1, 0, 1, b"y")), step(None, 1, false));
    }

    #[test]
    fn abandon_set_returns_0_for_a_set_being_skipped() {
        // intentional-difference:rx-sdu-cap-1mib
        let mut joiner = small();
        joiner.push(fragment(1, 0, 0, b"abcde"));
        assert_eq!(joiner.abandon_set(), 0);
        // and the skip is gone with it
        assert_eq!(joiner.push(fragment(1, 0, 1, b"x")), step(None, 1, false));
    }

    #[test]
    fn id_zero_over_the_cap_abandons_an_open_set_and_refuses_the_new_one() {
        // intentional-difference:rx-sdu-cap-1mib
        let mut joiner = small();
        joiner.push(fragment(1, 0, 0, b"ab"));
        let refused = joiner.push(fragment(1, 0, 0, b"abcde"));
        assert_eq!(refused, step(None, 1, true));
    }
}
