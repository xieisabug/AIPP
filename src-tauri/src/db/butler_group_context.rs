//! Pure group delivery policy. Visibility and activation are deliberately separate.
//! Callers must persist the returned packet once; never re-render a historical packet.

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Speaker { User, Member(i64) }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicMessage {
    pub group: i64,
    pub seq: i64,
    pub speaker: Speaker,
    pub text: String,
}

/// Membership is an epoch: rejoining must not reuse an old member's context.
#[derive(Clone, Debug)]
pub struct Reader {
    pub group: i64,
    pub member: i64,
    pub joined_after: i64,
    pub delivered_through: i64,
    pub active: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputPacket {
    pub after: i64,
    pub through: i64,
    pub messages: Vec<PublicMessage>,
    pub required_replies: Vec<i64>,
}

/// Only committed PUBLIC messages belong here. Tool records have no conversion to
/// PublicMessage. This function does not advance a cursor or acknowledge a reply.
pub fn prepare_packet(
    reader: &Reader,
    upper: i64,
    public: &[PublicMessage],
    required_replies: &[i64],
) -> Result<InputPacket, String> {
    if !reader.active { return Err("成员已退出，不能投递群消息".into()); }
    if upper < reader.delivered_through || upper < reader.joined_after {
        return Err("群消息上界早于成员上下文位置".into());
    }
    let after = reader.delivered_through.max(reader.joined_after);
    let mut visible: Vec<_> = public.iter()
        .filter(|m| m.group == reader.group && m.seq > after && m.seq <= upper)
        .cloned().collect();
    visible.sort_by_key(|m| m.seq);
    if visible.windows(2).any(|pair| pair[0].seq == pair[1].seq) {
        return Err("公共消息序号重复，不能生成确定的输入包".into());
    }
    let mut replies = required_replies.to_vec();
    replies.sort_unstable();
    replies.dedup();
    // Earlier references must be loaded and authorized separately, not inferred
    // from a cursor. This packet API only accepts triggers present in this delta.
    if replies.iter().any(|id| !visible.iter().any(|m| m.seq == *id)) {
        return Err("待回应消息不在本次可见增量中，需显式补读引用".into());
    }
    visible.retain(|m| m.speaker != Speaker::Member(reader.member));
    Ok(InputPacket { after, through: upper, messages: visible, required_replies: replies })
}

#[derive(Clone, Debug)]
pub enum Activation {
    UserPlain,
    Directed(Vec<i64>),
    Ordered { members: Vec<i64>, next: usize },
    DependencyReady { waiting_member: i64 },
    Background,
}

/// `active_members` must come from current database membership, never model args.
pub fn recipients(event: &Activation, chief: i64, active_members: &[i64]) -> Result<Vec<i64>, String> {
    let candidates = match event {
        Activation::UserPlain => vec![chief],
        Activation::Directed(ids) => ids.clone(),
        Activation::Ordered { members, next } => {
            if *next >= members.len() { return Err("发言席位不存在或讨论已结束".into()); }
            vec![members[*next]]
        },
        Activation::DependencyReady { waiting_member } => vec![*waiting_member],
        Activation::Background => Vec::new(),
    };
    let mut result = Vec::new();
    for member in candidates {
        if !active_members.contains(&member) { return Err(format!("成员 {member} 不在本群，不能唤醒")); }
        if !result.contains(&member) { result.push(member); }
    }
    Ok(result)
}

#[cfg(test)]
#[path = "butler_group_context_tests.rs"]
mod tests;
