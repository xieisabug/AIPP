use super::*;

fn reader() -> Reader {
    Reader { group: 1, member: 10, joined_after: 2, delivered_through: 2, active: true }
}
fn message(group: i64, seq: i64, speaker: Speaker) -> PublicMessage {
    PublicMessage { group, seq, speaker, text: format!("message {seq}") }
}

#[test]
fn test_group_packet_filters_other_groups_prejoin_future_and_own_outputs() {
    let rows = vec![message(1, 1, Speaker::User), message(2, 3, Speaker::User),
        message(1, 3, Speaker::User), message(1, 4, Speaker::Member(10)),
        message(1, 5, Speaker::Member(20)), message(1, 6, Speaker::User)];
    let packet = prepare_packet(&reader(), 5, &rows, &[3]).unwrap();
    assert_eq!(packet.messages.iter().map(|m|m.seq).collect::<Vec<_>>(), vec![3,5]);
    assert_eq!(packet.required_replies, vec![3]);
    assert_eq!(reader().delivered_through, 2);
}

#[test]
fn test_group_packet_snapshot_stays_unchanged_when_new_messages_arrive() {
    let mut rows = vec![message(1,3,Speaker::User)];
    let original = prepare_packet(&reader(),3,&rows,&[3]).unwrap();
    rows.push(message(1,4,Speaker::Member(20)));
    assert_eq!(prepare_packet(&reader(),3,&rows,&[3]).unwrap(), original);
    let mut next = reader(); next.delivered_through = 3;
    let delta = prepare_packet(&next,4,&rows,&[]).unwrap();
    assert_eq!(delta.messages.len(),1);
    assert_eq!(delta.messages[0].seq,4);
    assert!(delta.required_replies.is_empty());
}

#[test]
fn test_group_packet_rejects_removed_member_and_invisible_reply() {
    let rows = vec![message(1,3,Speaker::User)];
    assert!(prepare_packet(&reader(),3,&rows,&[1]).is_err());
    let mut removed = reader(); removed.active = false;
    assert!(prepare_packet(&removed,3,&rows,&[]).is_err());
    assert!(prepare_packet(&reader(),1,&rows,&[]).is_err());
}

#[test]
fn test_group_routing_background_never_wakes_and_ordered_only_wakes_current() {
    let members = [10,20,30];
    assert!(recipients(&Activation::Background,10,&members).unwrap().is_empty());
    assert_eq!(recipients(&Activation::UserPlain,10,&members).unwrap(),vec![10]);
    assert_eq!(recipients(&Activation::Ordered { members: members.to_vec(), next:1 },10,&members).unwrap(),vec![20]);
    assert_eq!(recipients(&Activation::Directed(vec![20,20,30]),10,&members).unwrap(),vec![20,30]);
    assert!(recipients(&Activation::Directed(vec![40]),10,&members).is_err());
}
