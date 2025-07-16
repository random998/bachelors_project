use poker_core::protocol::msg::*;
use poker_core::protocol::state::*;

#[test]
fn appends_keep_hash_consistent() {
    let kp = poker_core::crypto::KeyPair::default();
    let mut st = ContractState::default();
    let mut head = GENESIS_HASH.clone();

    for i in 0..3 {
        let msg = WireMsg::Throttle { millis: i };
        let next = step(&st, &msg, &PeerContext::new(kp.peer_id(), "n".into(), poker_core::poker::Chips::ZERO)).next;
        let h   = hash_state(&next);
        let le  = LogEntry::with_key(head.clone(), msg, h.clone(), kp.peer_id());
        head = le.next_hash.clone();
        st   = next;
    }
    assert_eq!(head, hash_state(&st));
}
