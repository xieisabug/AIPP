use super::*;

#[test]
fn test_model_catalog_antigravity_reopen_and_failed_refresh_preserve_account_models() {
    let db = LLMDatabase { conn: Connection::open_in_memory().unwrap() };
    db.create_tables().unwrap();
    let first = Catalog::bind(&db.conn, 7, "account-a").unwrap();
    let models = vec![("a".into(), "Account A".into())];
    first.save(&db.conn, &models).unwrap();
    let reopened = Catalog::bind(&db.conn, 7, "account-a").unwrap();
    assert!(reopened.save(&db.conn, &vec![]).is_err());
    assert_eq!(reopened.read(&db.conn).unwrap(), Some(models.clone()));
    assert!(Catalog::bind(&db.conn, 8, "account-a").unwrap().read(&db.conn).unwrap().is_none());
    let changed = Catalog::bind(&db.conn, 7, "account-b").unwrap();
    assert!(first.save(&db.conn, &models).is_err());
    assert!(changed.read(&db.conn).unwrap().is_none());
}
