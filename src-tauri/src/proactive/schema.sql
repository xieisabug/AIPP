CREATE TABLE IF NOT EXISTS butler_proactive_cursor (
    conversation_id INTEGER PRIMARY KEY REFERENCES conversation(id) ON DELETE CASCADE,
    pulse_position INTEGER NOT NULL DEFAULT 0,
    digest_position INTEGER NOT NULL DEFAULT 0,
    pulse_offset INTEGER NOT NULL DEFAULT -1,
    digest_offset INTEGER NOT NULL DEFAULT -1
);
CREATE TABLE IF NOT EXISTS butler_proactive_run (
    id TEXT PRIMARY KEY,
    trigger_kind TEXT NOT NULL,
    status TEXT NOT NULL,
    summary TEXT NOT NULL DEFAULT '',
    scanned INTEGER NOT NULL DEFAULT 0,
    conversation_id INTEGER,
    started_at INTEGER NOT NULL,
    finished_at INTEGER
);
CREATE INDEX IF NOT EXISTS idx_proactive_run_time ON butler_proactive_run(started_at);
CREATE TABLE IF NOT EXISTS butler_proactive_item (
    id INTEGER PRIMARY KEY,
    uuid TEXT NOT NULL UNIQUE,
    kind TEXT NOT NULL CHECK(kind IN ('open_loop','milestone','prep_topic','proposal')),
    title TEXT NOT NULL,
    detail TEXT NOT NULL DEFAULT '',
    tracking_summary TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'active',
    source_conversation_id INTEGER NOT NULL,
    source_position INTEGER NOT NULL DEFAULT 0,
    source_message_id INTEGER,
    source_group_post_id INTEGER,
    owner_assistant_id INTEGER,
    event_date TEXT,
    calendar_type TEXT NOT NULL DEFAULT 'solar',
    yearly INTEGER NOT NULL DEFAULT 0,
    lead_days INTEGER NOT NULL DEFAULT 0,
    high_priority INTEGER NOT NULL DEFAULT 0,
    next_check_at TEXT,
    action_prompt TEXT NOT NULL DEFAULT '',
    prep_message_id INTEGER,
    revision INTEGER NOT NULL DEFAULT 1,
    updated_at INTEGER NOT NULL,
    UNIQUE(source_message_id,kind,title),
    UNIQUE(source_group_post_id,kind,title)
);
CREATE TABLE IF NOT EXISTS butler_proactive_delivery_queue (
    id INTEGER PRIMARY KEY,
    occurrence_key TEXT NOT NULL UNIQUE,
    item_id INTEGER NOT NULL REFERENCES butler_proactive_item(id) ON DELETE CASCADE,
    occurrence TEXT NOT NULL,
    stage TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'queued',
    deliver_after INTEGER NOT NULL,
    morning INTEGER NOT NULL DEFAULT 0,
    researched INTEGER NOT NULL DEFAULT 0,
    summary TEXT NOT NULL DEFAULT '',
    brief_message_id INTEGER,
    delivered_at INTEGER,
    conversation_id INTEGER,
    message_id INTEGER,
    fallback_reason TEXT,
    external_key TEXT,
    external_recipient TEXT,
    external_status TEXT,
    external_error TEXT,
    UNIQUE(item_id,occurrence,stage)
);
CREATE INDEX IF NOT EXISTS idx_proactive_queue_due ON butler_proactive_delivery_queue(status,deliver_after);
CREATE TABLE IF NOT EXISTS butler_proactive_wake_retry (
    queue_id INTEGER PRIMARY KEY REFERENCES butler_proactive_delivery_queue(id) ON DELETE CASCADE,
    retry_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS butler_proactive_memory_change (
    memory_id INTEGER PRIMARY KEY REFERENCES butler_memory(id) ON DELETE CASCADE,
    written_revision INTEGER NOT NULL,
    previous TEXT
);
CREATE TABLE IF NOT EXISTS butler_proactive_acceptance (
    uuid TEXT PRIMARY KEY REFERENCES butler_proactive_item(uuid) ON DELETE CASCADE,
    conversation_id INTEGER NOT NULL, assistant_id INTEGER NOT NULL, prompt TEXT NOT NULL,
    accepted_at INTEGER NOT NULL, status TEXT NOT NULL, message_id INTEGER, error TEXT
);
CREATE TABLE IF NOT EXISTS butler_proactive_evidence (
    conversation_id INTEGER NOT NULL REFERENCES conversation(id) ON DELETE CASCADE,
    source_position INTEGER NOT NULL, kind TEXT NOT NULL, quote TEXT NOT NULL,
    PRIMARY KEY(conversation_id,source_position,kind,quote)
);
CREATE TABLE IF NOT EXISTS butler_proactive_source (
    id INTEGER PRIMARY KEY,
    uuid TEXT NOT NULL UNIQUE,
    item_id INTEGER NOT NULL REFERENCES butler_proactive_item(id) ON DELETE CASCADE,
    conversation_id INTEGER NOT NULL REFERENCES conversation(id) ON DELETE CASCADE,
    message_id INTEGER REFERENCES message(id) ON DELETE CASCADE,
    group_post_id INTEGER REFERENCES butler_chat_group_message(id) ON DELETE CASCADE,
    quote TEXT NOT NULL,
    CHECK ((message_id IS NULL) != (group_post_id IS NULL)),
    UNIQUE(item_id,message_id,quote),
    UNIQUE(item_id,group_post_id,quote)
);
