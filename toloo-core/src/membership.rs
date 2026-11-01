//! Room membership reconstruction (spec §5.6.4).
//!
//! Replays committed channel `-2` events in canonical order. Callers must
//! supply verified envelopes in the order returned by a room sync.

use std::collections::HashSet;

use crate::envelope::innermost;
use crate::types::Envelope;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MembershipState {
    pub members: HashSet<String>,
    pub banned: HashSet<String>,
}

/// Rebuild membership from committed membership events.
///
/// Uncommitted (depth-1) events are ignored. Later events override earlier
/// ones; banned nodes stay out until an unban event.
pub fn from_events<'a, I>(events: I) -> MembershipState
where
    I: IntoIterator<Item = &'a Envelope>,
{
    let mut state = MembershipState::default();

    for envelope in events {
        if envelope.d.tc.is_none() {
            continue;
        }
        let d = &innermost(envelope).d;
        match d.t.as_str() {
            "room.join" => {
                if !state.banned.contains(&d.n) {
                    state.members.insert(d.n.clone());
                }
            }
            "room.leave" => {
                state.members.remove(&d.n);
            }
            "room.ban" => {
                if let Some(target) = content_str(d, "banned") {
                    state.members.remove(target);
                    state.banned.insert(target.to_owned());
                }
            }
            "room.unban" => {
                if let Some(target) = content_str(d, "unbanned") {
                    state.banned.remove(target);
                }
            }
            _ => {}
        }
    }

    state
}

fn content_str<'a>(d: &'a crate::types::DatumBody, key: &str) -> Option<&'a str> {
    d.c.as_ref()?.get(key)?.as_str()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::from_events;
    use crate::envelope::wrap_commit;
    use crate::fixtures::{mk_node, mk_room, mk_room_event};
    use crate::types::Envelope;

    fn commit(inner: Envelope, room: &crate::types::LocalRoom, tc: u64) -> Envelope {
        wrap_commit(inner, room, tc).unwrap()
    }

    #[test]
    fn replays_join_leave_ban_and_unban() {
        let room = mk_room();
        let arta = mk_node();
        let babak = mk_node();

        let events = vec![
            commit(
                mk_room_event(&arta, &room, "room.join", 100, json!({})),
                &room,
                100,
            ),
            commit(
                mk_room_event(&babak, &room, "room.join", 101, json!({})),
                &room,
                101,
            ),
            commit(
                mk_room_event(&arta, &room, "room.leave", 102, json!({})),
                &room,
                102,
            ),
            commit(
                mk_room_event(
                    &babak,
                    &room,
                    "room.ban",
                    103,
                    json!({ "banned": arta.sig.pub_key }),
                ),
                &room,
                103,
            ),
            commit(
                mk_room_event(
                    &babak,
                    &room,
                    "room.unban",
                    104,
                    json!({ "unbanned": arta.sig.pub_key }),
                ),
                &room,
                104,
            ),
        ];

        let state = from_events(&events);
        assert_eq!(state.members.len(), 1);
        assert!(state.members.contains(&babak.sig.pub_key));
        assert!(!state.members.contains(&arta.sig.pub_key));
        assert!(state.banned.is_empty());
    }

    #[test]
    fn banned_nodes_cannot_rejoin_until_unbanned() {
        let room = mk_room();
        let arta = mk_node();

        let events = vec![
            commit(
                mk_room_event(
                    &arta,
                    &room,
                    "room.ban",
                    100,
                    json!({ "banned": arta.sig.pub_key }),
                ),
                &room,
                100,
            ),
            commit(
                mk_room_event(&arta, &room, "room.join", 101, json!({})),
                &room,
                101,
            ),
        ];

        let state = from_events(&events);
        assert!(state.banned.contains(&arta.sig.pub_key));
        assert!(!state.members.contains(&arta.sig.pub_key));
    }

    #[test]
    fn uncommitted_events_are_ignored() {
        let room = mk_room();
        let arta = mk_node();
        let join = mk_room_event(&arta, &room, "room.join", 100, json!({}));

        let state = from_events([&join]);
        assert!(state.members.is_empty());
    }
}
