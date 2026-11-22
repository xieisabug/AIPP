use super::*;
use crate::db::butler_group as group;

struct Device {
    db: Connection,
    sync: Connection,
}

#[test]
fn test_proactive_item_and_night_queue_map_to_receiver_source() {
    let source=Device::new();
    let target=Device::new();
    source.db.execute_batch("INSERT INTO assistant(id,name) VALUES(72,'A');
        INSERT INTO conversation(id,name,assistant_id,conversation_kind) VALUES(41,'私聊',72,'butler_contact');
        INSERT INTO message(id,conversation_id,message_type,content) VALUES(83,41,'user','下周提醒我');
        INSERT INTO butler_proactive_item(id,uuid,kind,title,source_conversation_id,source_message_id,owner_assistant_id,updated_at)
        VALUES(64,'stable-item','open_loop','复查',41,83,72,1);
        INSERT INTO butler_proactive_source(uuid,item_id,conversation_id,message_id,quote)
        VALUES('stable-reference',64,41,83,'下周提醒我');
        INSERT INTO butler_proactive_delivery_queue(occurrence_key,item_id,occurrence,stage,deliver_after)
        VALUES('stable-occurrence',64,'2026-11-23','due',1234);").unwrap();
    let mut changes=source.changes();
    for kind in ["conversation.proactive_item","conversation.proactive_occurrence","conversation.proactive_source"] {
        assert!(changes.iter().any(|c|c.object_type==kind),"missing outgoing {kind}");
    }
    changes.reverse();
    target.receive(&changes);
    let result:(String,String,String,i64)=target.db.query_row("SELECT a.name,m.content,q.status,i.id FROM butler_proactive_item i
        JOIN assistant a ON a.id=i.owner_assistant_id JOIN message m ON m.id=i.source_message_id AND m.conversation_id=i.source_conversation_id
        JOIN butler_proactive_delivery_queue q ON q.item_id=i.id WHERE i.uuid='stable-item' AND q.occurrence_key='stable-occurrence'",[],
        |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).unwrap();
    assert_eq!((&result.0[..],&result.1[..],&result.2[..]),("A","下周提醒我","queued"));
    assert_ne!(result.3,64);
    let valid:bool=target.db.query_row("SELECT EXISTS(SELECT 1 FROM butler_proactive_source s
        JOIN message m ON m.id=s.message_id AND m.conversation_id=s.conversation_id AND m.content=s.quote
        WHERE s.uuid='stable-reference' AND s.item_id=?1)",[result.3],|r|r.get(0)).unwrap();
    assert!(valid);
}

impl Device {
    fn new() -> Self {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("PRAGMA foreign_keys=ON;
            CREATE TABLE assistant(id INTEGER PRIMARY KEY,name TEXT NOT NULL);
            CREATE TABLE assistant_appearance(id INTEGER PRIMARY KEY,assistant_id INTEGER UNIQUE,value TEXT);
            CREATE TABLE conversation(id INTEGER PRIMARY KEY,name TEXT,assistant_id INTEGER,created_time TEXT,updated_time TEXT,
                conversation_kind TEXT,parent_butler_conversation_id INTEGER,is_hidden_from_normal_chat_list INTEGER);
            CREATE TABLE message(id INTEGER PRIMARY KEY, conversation_id INTEGER,message_type TEXT,content TEXT,finish_time TEXT,metadata_json TEXT);
            ").unwrap();
        db.execute_batch("CREATE TABLE assistant_skill_config(id INTEGER PRIMARY KEY, assistant_id INTEGER,skill_identifier TEXT,is_enabled INTEGER,priority INTEGER,created_time TEXT,UNIQUE(assistant_id,skill_identifier));
            CREATE TABLE assistant_prompt(id INTEGER PRIMARY KEY, assistant_id INTEGER,prompt TEXT);
            CREATE TABLE assistant_prompt_param(id INTEGER PRIMARY KEY,assistant_id INTEGER,assistant_prompt_id INTEGER,param_name TEXT,param_type TEXT,param_value TEXT);
            CREATE TABLE butler_main_state(id INTEGER PRIMARY KEY,slot TEXT UNIQUE,butler_conversation_id INTEGER);
            ALTER TABLE message ADD COLUMN parent_id INTEGER;").unwrap();
        crate::db::butler_goal::schema(&db).unwrap();
        crate::db::butler_memory::schema(&db).unwrap();
        db.execute_batch(include_str!("../proactive/schema.sql")).unwrap();
        group::schema(&db).unwrap();
        let sync = Connection::open_in_memory().unwrap();
        sync.execute_batch(SYNC_SCHEMA_SQL).unwrap();
        Self { db, sync }
    }

    fn changes(&self) -> Vec<PullChange> {
        let mut changes = Vec::new();
        for spec in super::super::specs().into_iter().filter(|spec| {
            matches!(
                spec.object_type,
                "assistant"
                    | "assistant.appearance"
                    | "assistant.skill"
                    | "assistant.prompt"
                    | "assistant.prompt_param"
                    | "conversation.butler_main"
                    | "conversation.butler_goal"
                    | "conversation.butler_memory"
                    | "conversation.proactive_item"
                    | "conversation.proactive_occurrence"
                    | "conversation.proactive_source"
                    | "conversation"
                    | "conversation.message"
                    | "conversation.group_member"
                    | "conversation.group_message"
                    | "conversation.group_turn"
                    | "conversation.read_reply"
                    | "conversation.read_group_reply"
            )
        }) {
            let (snapshots, _) = read_local_snapshots_from_db(
                &self.db,
                &spec,
                &self.sync,
                &mut DanglingFkScanReport::default(),
                |kind, id| {
                    let parent =
                        super::super::specs().into_iter().find(|s| s.object_type == kind).unwrap();
                    row_exists_by_id(&self.db, &parent, id)
                },
            )
            .unwrap();
            for row in snapshots {
                let payload = serde_json::from_str(&row.payload_json).unwrap();
                changes.push(PullChange {
                    seq: changes.len() as i64 + 1,
                    event_id: Uuid::new_v4().to_string(),
                    device_id: "source".into(),
                    object_type: row.object_type,
                    object_id: row.object_id,
                    operation: "upsert".into(),
                    version: 1,
                    payload: Some(payload),
                    deleted_at: None,
                    created_at: now_string(),
                });
            }
        }
        changes
    }

    fn receive(&self, changes: &[PullChange]) {
        apply_pull_page(&self.sync, changes, |change| {
            let spec = super::super::specs()
                .into_iter()
                .find(|s| s.object_type == change.object_type)
                .unwrap();
            apply_change_to_db(&self.sync, &self.db, &spec, change, "receiver", |_| Ok(()))
        })
        .unwrap();
        let error: Option<String> = self
            .sync
            .query_row("SELECT error FROM sync_dead_letter LIMIT 1", [], |r| r.get(0))
            .optional()
            .unwrap();
        assert_eq!(error, None);
    }
}

#[test]
fn test_group_history_appearance_and_read_receipts_survive_device_id_remap() {
    let mut source = Device::new();
    source
        .db
        .execute_batch(
            "INSERT INTO assistant VALUES(17,'Writer');
        INSERT INTO assistant_appearance VALUES(3,17,'beret');",
        )
        .unwrap();
    let room = group::create(&mut source.db, "Design", &[(17, "Writer".into())]).unwrap();
    let m = group::active_members(&source.db, room.id).unwrap().remove(0);
    let question = group::post_user(&mut source.db, room.id, "request", "Question", None).unwrap();
    source.db.execute("INSERT INTO message(id,conversation_id,message_type,content,finish_time) VALUES(75,?1,'response','Answer','now')",[m.conversation_id]).unwrap();
    source.db.execute("INSERT INTO butler_chat_group_message(group_id,seq,sender_member,sender_assistant,sender_name,kind,body,reply_to,source_conversation_id,source_message_id)
        VALUES(?1,?2,?3,17,'Writer','message','Answer',?4,?5,75)",params![room.id,question.seq+1,m.id,question.seq,m.conversation_id]).unwrap();
    source.db.execute_batch("INSERT INTO butler_read_reply(message_id) VALUES(75);
        INSERT INTO butler_read_group_reply(message_id) SELECT id FROM butler_chat_group_message WHERE body='Answer';").unwrap();
    let target = Device::new();
    target
        .db
        .execute_batch(
            "INSERT INTO assistant VALUES(17,'Different');
        INSERT INTO conversation(id,name,conversation_kind) VALUES(88,'other','normal');
        INSERT INTO message(id,conversation_id,message_type,content,finish_time) VALUES(75,88,'response','Other unread','now');",
        )
        .unwrap();
    source.db.execute("INSERT INTO butler_chat_group_turn(group_id,member_id,trigger_seq,origin_seq,input_message_id,reply_seq,state)
        VALUES(?1,?2,?3,?3,75,?4,'done')",params![room.id,m.id,question.seq,question.seq+1]).unwrap();
    let mut changes = source.changes();
    changes.reverse();
    target.receive(&changes);
    let local_conversation: i64 = target
        .db
        .query_row("SELECT conversation_id FROM butler_chat_group_member LIMIT 1", [], |r| r.get(0))
        .unwrap();
    target.db.execute("INSERT INTO message(conversation_id,message_type,content,finish_time) VALUES(?1,'response','Offline reply','now')",[local_conversation]).unwrap();
    let offline_reply = target.db.last_insert_rowid();
    target.receive(&changes);
    let offline_read: bool = target
        .db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM butler_read_reply WHERE message_id=?1)",
            [offline_reply],
            |r| r.get(0),
        )
        .unwrap();
    assert!(!offline_read);
    let room = group::list(&target.db).unwrap().remove(0);
    let member = group::active_members(&target.db, room.id).unwrap().remove(0);
    assert_ne!(member.assistant_id, 17);
    let appearance: String = target
        .db
        .query_row(
            "SELECT value FROM assistant_appearance WHERE assistant_id=?1",
            [member.assistant_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(appearance, "beret");
    let msgs = group::messages(&target.db, room.id, None, 20).unwrap();
    let reply = msgs.iter().find(|m| m.body == "Answer").unwrap();
    assert_eq!(reply.reply_to, Some(question.seq));
    assert_eq!(reply.sender_member, Some(member.id));
    assert_eq!(reply.source_conversation_id, Some(member.conversation_id));
    assert_eq!(group::interaction_turns(&target.db, room.id).unwrap().len(), 1);
    assert!(group::pending_routes(&target.db).unwrap().is_empty());
    let read: i64 =
        target.db.query_row("SELECT message_id FROM butler_read_reply", [], |r| r.get(0)).unwrap();
    assert_eq!(Some(read), reply.source_message_id);
    assert_ne!(read, 75);
    let group_read: String = target.db.query_row("SELECT body FROM butler_read_group_reply r JOIN butler_chat_group_message m ON m.id=r.message_id",[],|r|r.get(0)).unwrap();
    assert_eq!(group_read, "Answer");
}

#[test]
fn test_offline_positions_keep_both_messages_and_their_own_reply_targets() {
    let mut device = Device::new();
    let room = group::create(&mut device.db, "Group", &[(17, "Writer".into())]).unwrap();
    let first = group::post_user(&mut device.db, room.id, "first", "Local post", None).unwrap();
    let mut fields =
        Map::from_iter([("group_id".into(), json!(room.id)), ("seq".into(), json!(first.seq))]);
    let mut source_refs = Map::new();
    encode_positions(&device.db, "conversation.group_message", &mut fields, &mut source_refs)
        .unwrap();
    let collision = Map::from_iter([("seq".into(), json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"))]);
    decode_positions(&device.db, "conversation.group_message", &mut fields, &collision).unwrap();
    let second = fields["seq"].as_i64().unwrap();
    assert_ne!(second, first.seq);
    device.db.execute("INSERT INTO butler_chat_group_message(group_id,seq,sender_name,kind,body) VALUES(?1,?2,'Other','message','Remote post')",params![room.id,second]).unwrap();
    for (refs, expected) in [(&source_refs, first.seq), (&collision, second)] {
        let mut quote = Map::from_iter([
            ("group_id".into(), json!(room.id)),
            ("reply_to".into(), json!(first.seq)),
        ]);
        let wire = Map::from_iter([("reply_to".into(), refs["seq"].clone())]);
        decode_positions(&device.db, "conversation.group_message", &mut quote, &wire).unwrap();
        assert_eq!(quote["reply_to"], expected);
        let mut encoded = Map::new();
        encode_positions(&device.db, "conversation.group_message", &mut quote, &mut encoded)
            .unwrap();
        assert_eq!(encoded, wire);
        assert_eq!(quote["reply_to"], first.seq);
    }
}

#[test]
fn test_upgrade_composite_message_key_preserves_old_quotes_and_allocates_new_ids() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE conversation(id INTEGER PRIMARY KEY,name TEXT,conversation_kind TEXT);
        CREATE TABLE message(id INTEGER PRIMARY KEY);
        CREATE TABLE butler_chat_group_message(
            group_id INTEGER,seq INTEGER,sender_member INTEGER,sender_assistant INTEGER,sender_name TEXT,
            kind TEXT,body TEXT,reply_to INTEGER,request_key TEXT,route_state TEXT,created_at TEXT,
            PRIMARY KEY(group_id,seq));
        INSERT INTO conversation VALUES(5,'Studio','butler_group');
        INSERT INTO butler_chat_group_message(group_id,seq,sender_name,kind,body,created_at)
            VALUES(5,1,'User','message','First','2025-01-01');
        INSERT INTO butler_chat_group_message(group_id,seq,sender_name,kind,body,reply_to,created_at)
            VALUES(5,2,'User','message','Second',1,'2025-01-02');
        CREATE TABLE butler_chat_group(id INTEGER PRIMARY KEY,name TEXT,seq INTEGER,created_at TEXT,updated_at TEXT);
        INSERT INTO butler_chat_group VALUES(5,'Studio',2,'2025-01-01','2025-01-02');").unwrap();
    group::schema(&conn).unwrap();
    group::schema(&conn).unwrap();
    let (rows, unique_ids, quote): (i64, i64, i64) = conn
        .query_row(
            "SELECT count(*),count(DISTINCT id),max(reply_to) FROM butler_chat_group_message",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!((rows, unique_ids, quote), (2, 2, 1));
    let mapped: i64 = conn.query_row("SELECT count(DISTINCT uid) FROM butler_group_position WHERE group_id=5 AND wire_seq=local_seq",[],|r|r.get(0)).unwrap();
    assert_eq!(mapped, 2);
}

#[test]
fn test_settings_memory_current_main_and_goal_follow_portable_parents() {
    use crate::db::{butler_goal as goal, butler_memory as memory};
    let mut a = Device::new();
    a.db.execute_batch("INSERT INTO assistant VALUES(17,'Writer');
        INSERT INTO assistant_prompt VALUES(18,17,'Welcome {{name}}');
        INSERT INTO assistant_prompt_param VALUES(19,17,18,'name','string','Alice');
        INSERT INTO assistant_skill_config VALUES(3,17,'agents:design',0,4,'now');
        INSERT INTO conversation(id,name,assistant_id,conversation_kind) VALUES(12,'Current',17,'butler_main'),(13,'Archived',17,'butler_main');
        INSERT INTO butler_main_state VALUES(1,'default',12);").unwrap();
    goal::apply(&a.db, 12, "replace", "Write").unwrap();
    let room = group::create(&mut a.db, "Group", &[(17, "Writer".into())]).unwrap();
    let post = group::post_user(&mut a.db, room.id, "anchor", "Remember", None).unwrap();
    let group_memory = memory::remember(
        &a.db,
        memory::Scope::Group(room.id),
        "Team",
        Some(memory::Source::GroupMessage { group: room.id, seq: post.seq }),
        Some(17),
    )
    .unwrap()
    .0;
    memory::remember(&a.db, memory::Scope::Assistant(17), "Private", None, Some(17)).unwrap();
    memory::remember(&a.db, memory::Scope::Global, "Global", None, None).unwrap();
    let b = Device::new();
    b.db.execute_batch(
        "INSERT INTO assistant VALUES(17,'Other');
        INSERT INTO assistant_prompt VALUES(18,17,'Unrelated');
        INSERT INTO conversation(id,name,conversation_kind) VALUES(99,'Old main','butler_main');
        INSERT INTO butler_main_state VALUES(4,'default',99);",
    )
    .unwrap();
    memory::remember(&b.db, memory::Scope::Assistant(17), "Unrelated", None, None).unwrap();
    let mut changes = a.changes();
    changes.reverse();
    b.receive(&changes);
    let writer: i64 =
        b.db.query_row("SELECT id FROM assistant WHERE name='Writer'", [], |r| r.get(0)).unwrap();
    let room_b = group::list(&b.db).unwrap().remove(0);
    assert_ne!(writer, 17);
    let configured:(String,i64,i64)=b.db.query_row("SELECT skill_identifier,is_enabled,priority FROM assistant_skill_config WHERE assistant_id=?1",[writer],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
    assert_eq!(configured, ("agents:design".into(), 0, 4));
    let param:String=b.db.query_row("SELECT p.param_value FROM assistant_prompt_param p JOIN assistant_prompt t ON t.id=p.assistant_prompt_id WHERE t.prompt='Welcome {{name}}' AND t.assistant_id=p.assistant_id AND p.assistant_id=?1",[writer],|r|r.get(0)).unwrap();
    assert_eq!(param, "Alice");
    let viewer = memory::Viewer::Group(room_b.id);
    let rows = memory::all_by_recency(&b.db, viewer).unwrap();
    assert_eq!(rows.len(), 2);
    let team = rows.iter().find(|r| r.content == "Team").unwrap();
    assert_eq!(team.author_assistant_id, Some(writer));
    assert_eq!(team.source_group_id, Some(room_b.id));
    assert_eq!(
        group::messages(&b.db, room_b.id, None, 20)
            .unwrap()
            .iter()
            .find(|m| Some(m.seq) == team.source_group_seq)
            .unwrap()
            .body,
        "Remember"
    );
    assert!(memory::all_by_recency(&b.db, memory::Viewer::Assistant(writer))
        .unwrap()
        .iter()
        .any(|x| x.content == "Private"));
    let main: i64 =
        b.db.query_row(
            "SELECT butler_conversation_id FROM butler_main_state WHERE slot='default'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_ne!(main, 12);
    assert_eq!(goal::current(&b.db, main).unwrap().unwrap().text, "Write");
    goal::apply(&b.db, main, "replace", "Local work").unwrap();
    let old = goal::current_revision(&b.db, main).unwrap().unwrap();
    goal::apply(&a.db, 12, "stop", "").unwrap();
    a.db.execute("UPDATE butler_main_state SET butler_conversation_id=13", []).unwrap();
    a.db.execute(
        "UPDATE butler_memory SET content='Updated team',revision=revision+1 WHERE id=?1",
        [group_memory.id],
    )
    .unwrap();
    a.db.execute_batch(
        "UPDATE assistant_skill_config SET is_enabled=1, priority=2;
        UPDATE assistant_prompt_param SET param_value='Bob';",
    )
    .unwrap();
    let mut updates = a.changes();
    for change in &mut updates {
        change.version = 2;
        change.seq += 1000;
    }
    b.receive(&updates);
    b.receive(&updates);
    let config: (i64, i64) =
        b.db.query_row(
            "SELECT is_enabled,priority FROM assistant_skill_config WHERE assistant_id=?1",
            [writer],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(config, (1, 2));
    assert_eq!(
        b.db.query_row(
            "SELECT param_value FROM assistant_prompt_param WHERE assistant_id=?1",
            [writer],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "Bob"
    );
    assert!(memory::all_by_recency(&b.db, viewer)
        .unwrap()
        .iter()
        .any(|m| m.content == "Updated team" && m.revision == 2));
    let stopped = goal::current(&b.db, main).unwrap().unwrap();
    assert!(stopped.stopped && stopped.revision > old);
    assert!(goal::assert_current(&b.db, main, old).is_err());
    let name:String=b.db.query_row("SELECT c.name FROM butler_main_state s JOIN conversation c ON c.id=s.butler_conversation_id",[],|r|r.get(0)).unwrap();
    assert_eq!(name, "Archived");
    let mut tombstone = updates
        .into_iter()
        .find(|c| {
            c.object_type == "conversation.butler_memory"
                && c.payload.as_ref().unwrap()["fields"]["content"] == "Updated team"
        })
        .unwrap();
    let spec = specs().into_iter().find(|s| s.object_type == "conversation.butler_memory").unwrap();
    for mut accepted in a.changes().into_iter().filter(|c| c.object_type == spec.object_type) {
        accepted.version = tombstone.version;
        upsert_shadow_from_remote(&a.sync, &accepted).unwrap();
    }
    let scan = || incremental::scan(&a.db, &a.sync, &spec, "source", None, &mut Default::default(),
        |_, _| Ok(true), |object| local_row_exists_in_db(&a.sync, &a.db, &spec, object)).unwrap();
    assert_eq!(scan(), 0);
    assert!(memory::forget(&a.db, Some(memory::Viewer::Group(room.id)), group_memory.id, Some(2)).unwrap());
    assert_eq!(scan(), 1);
    let (_, event) = load_pending_outbox_from_db(&a.sync, 10).unwrap().pop().unwrap();
    assert_eq!(event.object_id, tombstone.object_id);
    assert_eq!(event.base_version, Some(tombstone.version));
    assert_eq!(event.operation, "delete");
    tombstone.seq += 1000;
    tombstone.version += 1;
    tombstone.operation = event.operation;
    tombstone.payload = event.payload;
    tombstone.deleted_at = Some(event.created_at);
    b.receive(&[tombstone]);
    assert_eq!(memory::all_by_recency(&b.db, viewer).unwrap().len(), 1);
}
