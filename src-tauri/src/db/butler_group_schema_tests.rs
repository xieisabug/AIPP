use super::*;

fn legacy(constraint:&str)->Connection {
    let conn=Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys=ON;
        CREATE TABLE conversation(id INTEGER PRIMARY KEY);
        CREATE TABLE message(id INTEGER PRIMARY KEY);").unwrap();
    let sql=include_str!("butler_group_schema.sql")
        .replace("    conversation_id INTEGER NOT NULL REFERENCES conversation(id),\n","")
        .replace("    conversation_id INTEGER NOT NULL REFERENCES conversation(id),\r\n","")
        .replace("UNIQUE(conversation_id,event_seq,reason)",constraint);
    conn.execute_batch(&sql).unwrap();
    conn.execute_batch("INSERT INTO conversation VALUES(1),(2),(3);
        INSERT INTO butler_group(id,name,goal,seq) VALUES(1,'旧群','目标',1);
        INSERT INTO butler_group_member(id,group_id,assistant_id,name,conversation_id,joined_after,delivered_through)
            VALUES(7,1,8,'成员',2,0,1);
        INSERT INTO butler_group_event(group_id,seq,sender_name,kind,body,request_key,request_json)
            VALUES(1,1,'用户','message','原要求','request','{}');
        INSERT INTO butler_group_packet(id,member_id,after_seq,through_seq,payload,state)
            VALUES(9,7,0,1,'immutable payload','running');
        ALTER TABLE butler_group_inbox ADD COLUMN snapshot_seq INTEGER;
        INSERT INTO butler_group_inbox(id,group_id,member_id,event_seq,reason,state,packet_id,error,snapshot_seq)
            VALUES(11,1,7,1,'direct','waiting',9,'existing diagnostic',1);
        INSERT INTO butler_group_preparation(id,member_id,after_seq,through_seq,body_json,items_json,shares_json,state)
            VALUES(12,7,0,1,'{}','[]','[]','completed');
        INSERT INTO butler_group_dependency(id,owner_packet,target_member,request_seq,state)
            VALUES(13,9,7,1,'pending');").unwrap();
    conn
}

#[test]
fn test_group_schema_migrates_parsed_unique_constraint_and_preserves_execution_data() {
    for constraint in ["UNIQUE(member_id,event_seq,reason)","unique(\"reason\", \"MEMBER_ID\", \"event_seq\")"] {
        let conn=legacy(constraint);
        schema(&conn).unwrap();
        schema(&conn).unwrap();
        assert!(!has_legacy_inbox_uniqueness(&conn).unwrap());
        let inbox:(i64,String,i64,String,i64)=conn.query_row(
            "SELECT conversation_id,state,packet_id,error,snapshot_seq FROM butler_group_inbox WHERE id=11",[],
            |row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?))).unwrap();
        assert_eq!(inbox,(2,"waiting".into(),9,"existing diagnostic".into(),1));
        assert_eq!(conn.query_row("SELECT payload FROM butler_group_packet WHERE id=9",[],|r|r.get::<_,String>(0)).unwrap(),"immutable payload");
        for sql in ["SELECT conversation_id FROM butler_group_packet WHERE id=9",
            "SELECT conversation_id FROM butler_group_preparation WHERE id=12",
            "SELECT target_conversation_id FROM butler_group_dependency WHERE id=13"] {
            assert_eq!(conn.query_row(sql,[],|r|r.get::<_,i64>(0)).unwrap(),2);
        }
        assert_eq!(conn.query_row("SELECT delivered_through FROM butler_group_execution WHERE conversation_id=2",[],|r|r.get::<_,i64>(0)).unwrap(),1);
        conn.execute("INSERT INTO butler_group_execution(conversation_id,member_id,kind,delivered_through) VALUES(3,7,'task',1)",[]).unwrap();
        conn.execute("INSERT INTO butler_group_inbox(group_id,member_id,conversation_id,event_seq,reason) VALUES(1,7,3,1,'direct')",[]).unwrap();
        assert!(conn.execute("INSERT INTO butler_group_inbox(group_id,member_id,conversation_id,event_seq,reason) VALUES(1,7,3,1,'direct')",[]).is_err());
        conn.execute("UPDATE butler_group_execution SET state='cancelled' WHERE conversation_id=3",[]).unwrap();
        schema(&conn).unwrap();
        assert_eq!(conn.query_row("SELECT state FROM butler_group_execution WHERE conversation_id=3",[],|r|r.get::<_,String>(0)).unwrap(),"cancelled");
        assert_eq!(conn.query_row("SELECT count(*) FROM butler_group_inbox WHERE conversation_id=3",[],|r|r.get::<_,i64>(0)).unwrap(),1);
        assert_eq!(conn.query_row("SELECT count(*) FROM pragma_foreign_key_check",[],|r|r.get::<_,i64>(0)).unwrap(),0);
    }
}

#[test]
fn test_group_schema_rolls_back_all_migration_changes_when_active_identity_conflicts() {
    let conn=legacy("UNIQUE(member_id,event_seq,reason)");
    conn.execute("INSERT INTO butler_group_packet(id,member_id,after_seq,through_seq,payload,state) VALUES(10,7,0,1,'conflicting packet','running')",[]).unwrap();
    assert!(schema(&conn).is_err());
    assert!(has_legacy_inbox_uniqueness(&conn).unwrap());
    assert_eq!(conn.query_row("SELECT count(*) FROM pragma_table_info('butler_group_packet') WHERE name='conversation_id'",[],|r|r.get::<_,i64>(0)).unwrap(),0);
    assert_eq!(conn.query_row("SELECT count(*) FROM butler_group_execution",[],|r|r.get::<_,i64>(0)).unwrap(),0);
    assert_eq!(conn.query_row("SELECT state FROM butler_group_inbox WHERE id=11",[],|r|r.get::<_,String>(0)).unwrap(),"waiting");
    conn.execute("DELETE FROM butler_group_packet WHERE id=10",[]).unwrap();
    schema(&conn).unwrap();
    assert!(!has_legacy_inbox_uniqueness(&conn).unwrap());
}
