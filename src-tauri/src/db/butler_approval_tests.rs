use super::*;
fn binding()->ApprovalBinding{ApprovalBinding{conversation:5,action:"send".into(),args:serde_json::json!({"recipient":"a","resource":"r"}),goal_revision:Some(1),latest_user:Some(8),expires_at:100}}
#[test] fn test_approval_binding_rejects_changed_target_resource_goal_and_scope(){
 let original=binding();assert!(original.validate(&original,99).is_ok());
 for field in ["recipient","resource"] {let mut changed=original.clone();changed.args[field]=serde_json::json!("other");assert!(original.validate(&changed,99).is_err());}
 let mut changed=original.clone();changed.goal_revision=Some(2);assert!(original.validate(&changed,99).is_err());
 changed=original.clone();changed.conversation=6;assert!(original.validate(&changed,99).is_err());
 changed=original.clone();changed.latest_user=Some(9);assert!(original.validate(&changed,99).is_err());
}
#[test] fn test_approval_binding_expires_at_deadline(){let b=binding();assert!(b.validate(&b,100).is_err());}

#[test]
fn test_operation_approval_cannot_borrow_latest_goal() {
 assert_eq!(bound_goal_revision(Some(2),Some(2)).unwrap(),Some(2));
 assert!(bound_goal_revision(Some(2),Some(1)).is_err());
 assert!(bound_goal_revision(Some(2),None).is_err());
 assert_eq!(bound_goal_revision(None,None).unwrap(),None);
}
#[test]
fn test_operation_permission_snapshot_rejects_goal_user_path_and_expiry() {
 let original=ApprovalBinding { action:"operation.write_file".into(),args:serde_json::json!({"path":"/tmp/result"}),..binding() };
 let mut current=original.clone(); current.goal_revision=Some(2);
 assert!(original.validate(&current,99).is_err());
 current=original.clone();current.latest_user=Some(10);
 assert!(original.validate(&current,99).is_err());
 current=original.clone();current.args=serde_json::json!({"path":"/tmp/other"});
 assert!(original.validate(&current,99).is_err());
 assert!(original.validate(&original,100).is_err());
 assert!(original.validate(&original,99).is_ok());
}

fn native_fixture() -> (rusqlite::Connection, NativeApprovalGuard) {
 let conn=rusqlite::Connection::open_in_memory().unwrap();
 conn.execute_batch("CREATE TABLE conversation(id INTEGER PRIMARY KEY,conversation_kind TEXT,butler_task_status TEXT);
 CREATE TABLE message(id INTEGER PRIMARY KEY,conversation_id INTEGER,message_type TEXT,parent_id INTEGER);
 INSERT INTO conversation VALUES(5,'butler_task','running');
 INSERT INTO message VALUES(10,5,'user',NULL),(11,5,'response',10);").unwrap();
 crate::db::butler_goal::schema(&conn).unwrap();
 conn.execute("INSERT INTO butler_goal VALUES(5,1,'work',0)",[]).unwrap();
 crate::db::butler_goal::bind_turn(&conn,5,10,1).unwrap();
 let request=ApprovalRequest{action_id:"native.permission".into(),args:serde_json::json!({"run":"run-1","command":"write approved file"})};
 let frozen=binding_from_connection(&conn,5,&request,100,Some(11)).unwrap();
 (conn,NativeApprovalGuard{frozen,request,source:Some(11)})
}
#[test]
fn test_native_permission_rejects_pause_cancel_and_expiry() {
 let (conn,guard)=native_fixture();
 assert!(guard.validate_connection(&conn,99).is_ok());
 crate::db::butler_control::set(&conn,5,"pause").unwrap();
 assert!(guard.validate_connection(&conn,99).is_err());
 crate::db::butler_control::set(&conn,5,"resume").unwrap();
 assert!(guard.validate_connection(&conn,100).is_err());
 crate::db::butler_control::set(&conn,5,"cancel").unwrap();
 assert!(guard.validate_connection(&conn,99).is_err());
}
#[test]
fn test_native_permission_keeps_original_source_after_new_goal() {
 let (conn,guard)=native_fixture();
 conn.execute("UPDATE butler_goal SET revision=2 WHERE conversation_id=5",[]).unwrap();
 conn.execute("INSERT INTO message VALUES(20,5,'user',NULL),(21,5,'response',20)",[]).unwrap();
 crate::db::butler_goal::bind_turn(&conn,5,20,2).unwrap();
 assert!(guard.validate_connection(&conn,99).is_err());
 assert!(binding_from_connection(&conn,5,&guard.request,100,Some(11)).is_err());
 assert!(binding_from_connection(&conn,5,&guard.request,100,None).is_err());
 assert!(binding_from_connection(&conn,5,&guard.request,100,Some(21)).is_ok());
}
#[test]
fn test_native_permission_rejects_new_user_message_and_terminal_work() {
 let (conn,guard)=native_fixture();
 conn.execute("INSERT INTO message VALUES(12,5,'user',NULL)",[]).unwrap();
 assert!(guard.validate_connection(&conn,99).is_err());
 conn.execute("DELETE FROM message WHERE id=12",[]).unwrap();
 conn.execute("UPDATE conversation SET butler_task_status='cancelled' WHERE id=5",[]).unwrap();
 assert!(guard.validate_connection(&conn,99).is_err());
}
