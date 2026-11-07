use super::*;
#[test] fn test_progress_notification_deduplicates_without_cross_task_loss(){let c=Connection::open_in_memory().unwrap();assert!(claim_notification(&c,1,"task_terminal","完成","结果",false,None).unwrap());assert!(!claim_notification(&c,1,"task_terminal","完成","结果",false,None).unwrap());assert!(claim_notification(&c,2,"task_terminal","完成","结果",false,None).unwrap());assert!(claim_notification(&c,1,"task_terminal","完成","新结果",false,None).unwrap());}
#[test] fn test_progress_quiet_keeps_results_and_questions(){let c=Connection::open_in_memory().unwrap();assert!(!claim_notification(&c,1,"progress","进展","无变化",false,Some("quiet")).unwrap());assert!(claim_notification(&c,1,"question","问题","请回答",true,Some("quiet")).unwrap());assert!(claim_notification(&c,1,"task_terminal","结果","已完成",false,Some("quiet")).unwrap());}
#[test] fn test_progress_references_reject_foreign_message(){let c=Connection::open_in_memory().unwrap();c.execute_batch("CREATE TABLE message(id INTEGER,conversation_id INTEGER);CREATE TABLE message_attachment(id INTEGER,message_id INTEGER,attachment_url TEXT);INSERT INTO message VALUES(1,10),(2,20);INSERT INTO message_attachment VALUES(3,1,'file.txt'),(4,2,'secret.txt')").unwrap();let(e,a)=result_references(&c,10,Some(2)).unwrap();assert!(e.is_empty());assert_eq!(a.len(),1);assert_eq!(a[0]["location"],"file.txt");let(e,_)=result_references(&c,10,Some(1)).unwrap();assert_eq!(e[0]["message_id"],1);}

#[test]
fn test_progress_retains_failure_reason_and_paused_next_step_after_reload() {
 let c=Connection::open_in_memory().unwrap();
 c.execute_batch("CREATE TABLE conversation(id INTEGER PRIMARY KEY);CREATE TABLE butler_task_result(task_conversation_id INTEGER,evidence_json TEXT,artifact_refs_json TEXT,summary TEXT);INSERT INTO butler_task_result VALUES(1,'[]','[]','读取指定文件失败：文件不存在')").unwrap();
 let failed=read(&c,1,"failed",false,false).unwrap();
 assert_eq!(failed.blocker.as_deref(),Some("读取指定文件失败：文件不存在"));
 super::super::butler_control::set(&c,2,"pause").unwrap();
 let paused=read(&c,2,"running",false,false).unwrap();
 assert!(paused.next_step.contains("由你恢复"));assert!(paused.blocker.unwrap().contains("已暂停"));
 assert!(!read(&c,3,"running",false,false).unwrap().next_step.contains("恢复"));
}
