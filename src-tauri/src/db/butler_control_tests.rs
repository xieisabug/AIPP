use super::*;
#[test] fn test_control_pause_persists_and_resume_does_not_restore_delegation() {
 let c=Connection::open_in_memory().unwrap();
 set(&c,1,"stop_delegation").unwrap(); set(&c,1,"pause").unwrap();
 assert!(ensure_turn(&c,1).is_err()); assert!(ensure_turn(&c,2).is_ok());
 set(&c,1,"resume").unwrap(); assert!(ensure_turn(&c,1).is_ok());assert!(ensure_delegation(&c,1).is_err());
}
#[test] fn test_control_cancel_is_irreversible_and_other_work_remains_available() {
 let c=Connection::open_in_memory().unwrap();set(&c,1,"cancel").unwrap();
 assert!(set(&c,1,"resume").is_err());assert!(ensure_turn(&c,1).is_err());assert!(ensure_delegation(&c,2).is_ok());
}
