CREATE TABLE IF NOT EXISTS butler_group (
    id INTEGER PRIMARY KEY REFERENCES conversation(id) ON DELETE CASCADE,
    name TEXT NOT NULL, goal TEXT NOT NULL, chief_member INTEGER,
    seq INTEGER NOT NULL DEFAULT 0, state TEXT NOT NULL DEFAULT 'active'
        CHECK(state IN ('active','stopped'))
);
CREATE TABLE IF NOT EXISTS butler_group_member (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    group_id INTEGER NOT NULL REFERENCES butler_group(id) ON DELETE CASCADE,
    assistant_id INTEGER NOT NULL, name TEXT NOT NULL,
    conversation_id INTEGER NOT NULL UNIQUE REFERENCES conversation(id) ON DELETE CASCADE,
    joined_after INTEGER NOT NULL, delivered_through INTEGER NOT NULL,
    active INTEGER NOT NULL DEFAULT 1 CHECK(active IN (0,1))
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_butler_group_active_member
    ON butler_group_member(group_id,assistant_id) WHERE active=1;
CREATE TABLE IF NOT EXISTS butler_group_event (
    group_id INTEGER NOT NULL REFERENCES butler_group(id) ON DELETE CASCADE,
    seq INTEGER NOT NULL, sender_member INTEGER REFERENCES butler_group_member(id),
    sender_name TEXT NOT NULL,
    kind TEXT NOT NULL CHECK(kind IN ('message','progress','result','question','notice')),
    body TEXT NOT NULL, reply_to INTEGER,
    request_key TEXT NOT NULL, request_json TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY(group_id,seq), UNIQUE(group_id,request_key),
    FOREIGN KEY(group_id,reply_to) REFERENCES butler_group_event(group_id,seq)
);
CREATE TABLE IF NOT EXISTS butler_group_share (
    member_id INTEGER NOT NULL REFERENCES butler_group_member(id) ON DELETE CASCADE,
    event_seq INTEGER NOT NULL, shared_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY(member_id,event_seq)
);
CREATE TABLE IF NOT EXISTS butler_group_round (
    id INTEGER PRIMARY KEY, group_id INTEGER NOT NULL REFERENCES butler_group(id) ON DELETE CASCADE,
    event_seq INTEGER NOT NULL, request_key TEXT NOT NULL, members_json TEXT NOT NULL,
    next_slot INTEGER NOT NULL DEFAULT 0,
    state TEXT NOT NULL DEFAULT 'active' CHECK(state IN ('active','completed','cancelled')),
    UNIQUE(group_id,request_key),
    FOREIGN KEY(group_id,event_seq) REFERENCES butler_group_event(group_id,seq)
);
CREATE TABLE IF NOT EXISTS butler_group_inbox (
    id INTEGER PRIMARY KEY, group_id INTEGER NOT NULL REFERENCES butler_group(id) ON DELETE CASCADE,
    member_id INTEGER NOT NULL REFERENCES butler_group_member(id) ON DELETE CASCADE,
    conversation_id INTEGER NOT NULL REFERENCES conversation(id),
    event_seq INTEGER NOT NULL, reason TEXT NOT NULL, round_id INTEGER REFERENCES butler_group_round(id),
    state TEXT NOT NULL DEFAULT 'queued' CHECK(state IN ('queued','prepared','delivered','answered','waiting','failed','cancelled','uncertain')),
    packet_id INTEGER, error TEXT,
    UNIQUE(conversation_id,event_seq,reason),
    FOREIGN KEY(group_id,event_seq) REFERENCES butler_group_event(group_id,seq)
);
CREATE INDEX IF NOT EXISTS idx_butler_group_inbox_queue ON butler_group_inbox(member_id,state,id);
CREATE TABLE IF NOT EXISTS butler_group_packet (
    id INTEGER PRIMARY KEY, member_id INTEGER NOT NULL REFERENCES butler_group_member(id) ON DELETE CASCADE,
    after_seq INTEGER NOT NULL, through_seq INTEGER NOT NULL,
    payload TEXT NOT NULL, input_message_id INTEGER UNIQUE REFERENCES message(id),
    state TEXT NOT NULL DEFAULT 'prepared' CHECK(state IN ('prepared','delivered','running','completed','failed','cancelled','uncertain')),
    error TEXT, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE IF NOT EXISTS butler_group_packet_share (
    packet_id INTEGER NOT NULL REFERENCES butler_group_packet(id) ON DELETE CASCADE,
    member_id INTEGER NOT NULL, event_seq INTEGER NOT NULL,
    PRIMARY KEY(packet_id,event_seq),
    FOREIGN KEY(member_id,event_seq) REFERENCES butler_group_share(member_id,event_seq)
);
CREATE TABLE IF NOT EXISTS butler_group_stop_report (
    id INTEGER PRIMARY KEY, group_id INTEGER NOT NULL REFERENCES butler_group(id) ON DELETE CASCADE,
    member_id INTEGER NOT NULL REFERENCES butler_group_member(id),
    succeeded INTEGER NOT NULL, detail TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE IF NOT EXISTS butler_group_dependency (
    id INTEGER PRIMARY KEY,
    owner_packet INTEGER NOT NULL REFERENCES butler_group_packet(id),
    target_member INTEGER NOT NULL REFERENCES butler_group_member(id),
    request_seq INTEGER NOT NULL, result_seq INTEGER,
    state TEXT NOT NULL DEFAULT 'pending' CHECK(state IN ('pending','ready','delivered','cancelled')),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(owner_packet,target_member,request_seq)
);
CREATE TABLE IF NOT EXISTS butler_group_resume (
    packet_id INTEGER NOT NULL REFERENCES butler_group_packet(id),
    dependency_id INTEGER NOT NULL UNIQUE REFERENCES butler_group_dependency(id),
    PRIMARY KEY(packet_id,dependency_id)
);
CREATE TABLE IF NOT EXISTS butler_group_independent (
    id INTEGER PRIMARY KEY,
    group_id INTEGER NOT NULL REFERENCES butler_group(id),
    event_seq INTEGER NOT NULL, snapshot_seq INTEGER NOT NULL,
    request_key TEXT NOT NULL, members_json TEXT NOT NULL,
    UNIQUE(group_id,request_key)
);
-- A frozen preparation precedes any model summary call. It never advances the
-- member cursor and contains only that membership epoch's authorized input.
CREATE TABLE IF NOT EXISTS butler_group_preparation (
    id INTEGER PRIMARY KEY,
    member_id INTEGER NOT NULL REFERENCES butler_group_member(id),
    after_seq INTEGER NOT NULL, through_seq INTEGER NOT NULL,
    body_json TEXT NOT NULL, items_json TEXT NOT NULL, shares_json TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'pending' CHECK(state IN ('pending','completed','failed','cancelled')),
    error TEXT, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE IF NOT EXISTS butler_group_background_summary (
    id INTEGER PRIMARY KEY,
    preparation_id INTEGER NOT NULL REFERENCES butler_group_preparation(id),
    ordinal INTEGER NOT NULL, source_json TEXT NOT NULL,
    summary TEXT, model_code TEXT, provider_id INTEGER,
    UNIQUE(preparation_id,ordinal)
);
CREATE TABLE IF NOT EXISTS butler_group_supplement (
    id INTEGER PRIMARY KEY,
    packet_id INTEGER NOT NULL REFERENCES butler_group_packet(id),
    after_seq INTEGER NOT NULL, through_seq INTEGER NOT NULL,
    payload TEXT NOT NULL,
    input_message_id INTEGER NOT NULL UNIQUE REFERENCES message(id),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE IF NOT EXISTS butler_group_native_input (
    id INTEGER PRIMARY KEY,
    packet_id INTEGER NOT NULL REFERENCES butler_group_packet(id),
    plan_json TEXT NOT NULL,
    run_id TEXT NOT NULL, session_id TEXT NOT NULL, turn_id TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'sending' CHECK(state IN ('sending','delivered','uncertain','cancelled')),
    input_message_id INTEGER REFERENCES message(id), error TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_butler_group_native_input_sending
    ON butler_group_native_input(packet_id) WHERE state='sending';
CREATE TABLE IF NOT EXISTS butler_group_execution (
    conversation_id INTEGER PRIMARY KEY REFERENCES conversation(id),
    member_id INTEGER NOT NULL REFERENCES butler_group_member(id),
    kind TEXT NOT NULL CHECK(kind IN ('main','task')),
    title TEXT NOT NULL DEFAULT '',
    parent_packet INTEGER REFERENCES butler_group_packet(id),
    request_key TEXT, request_json TEXT,
    source_seqs_json TEXT NOT NULL DEFAULT '[]',
    delivered_through INTEGER NOT NULL,
    state TEXT NOT NULL DEFAULT 'active' CHECK(state IN ('active','cancelled')),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(member_id,request_key)
);
-- Every assistant-triggered delivery has an owner, including fire-and-forget
-- messages and discussion seats. Waiting for a result is a separate concern.
CREATE TABLE IF NOT EXISTS butler_group_dispatch (
    owner_packet INTEGER NOT NULL REFERENCES butler_group_packet(id),
    event_seq INTEGER NOT NULL,
    target_conversation_id INTEGER NOT NULL REFERENCES conversation(id),
    reason TEXT NOT NULL,
    PRIMARY KEY(owner_packet,event_seq,target_conversation_id,reason)
);
CREATE INDEX IF NOT EXISTS idx_butler_group_dispatch_target
    ON butler_group_dispatch(target_conversation_id,event_seq,reason);
CREATE TABLE IF NOT EXISTS butler_group_capability (
    member_id INTEGER PRIMARY KEY REFERENCES butler_group_member(id),
    fingerprint TEXT NOT NULL,
    profile_json TEXT NOT NULL,
    event_seq INTEGER NOT NULL
);
