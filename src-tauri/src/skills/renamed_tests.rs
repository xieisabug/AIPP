use super::*;
use tempfile::TempDir;

static RENAMED: &[RenamedSkill] = &[RenamedSkill {
    old: "old-skill",
    new: "new-skill",
    // sha256("official v1")
    official_digests: &["ad79d30cea489123dee7c905c80840ab3ebfd9154ececa7b7ffa6692359a5298"],
}];

fn write_skill(dir: &Path, folder: &str, content: &str) {
    fs::create_dir_all(dir.join(folder).join("references")).unwrap();
    fs::write(dir.join(folder).join("SKILL.md"), content).unwrap();
}

fn bindings_db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE assistant_skill_config(id INTEGER PRIMARY KEY, assistant_id INTEGER NOT NULL,
           skill_identifier TEXT NOT NULL, is_enabled BOOLEAN NOT NULL DEFAULT 1, priority INTEGER NOT NULL DEFAULT 0,
           UNIQUE(assistant_id, skill_identifier));
         INSERT INTO assistant_skill_config(assistant_id, skill_identifier, is_enabled) VALUES
           (1, 'agents:old-skill', 1), (2, 'agents:old-skill', 0), (2, 'agents:new-skill', 1), (3, 'agents:other', 1);",
    )
    .unwrap();
    conn
}

fn identifiers(conn: &Connection) -> Vec<(i64, String, bool)> {
    let mut stmt = conn
        .prepare("SELECT assistant_id, skill_identifier, is_enabled FROM assistant_skill_config ORDER BY assistant_id, skill_identifier")
        .unwrap();
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().map(Result::unwrap).collect()
}

#[test]
fn keeps_old_folder_until_the_replacement_is_installed() {
    let dir = TempDir::new().unwrap();
    write_skill(dir.path(), "old-skill", "official v1");
    assert!(retire_in(dir.path(), RENAMED).is_empty());
    assert!(dir.path().join("old-skill").exists());
}

#[test]
fn removes_unmodified_official_folder_once_replaced() {
    let dir = TempDir::new().unwrap();
    write_skill(dir.path(), "old-skill", "official v1");
    write_skill(dir.path(), "new-skill", "v2");
    assert_eq!(retire_in(dir.path(), RENAMED), vec!["old-skill"]);
    assert!(!dir.path().join("old-skill").exists());
    assert!(dir.path().join("new-skill/SKILL.md").is_file());
}

#[test]
fn keeps_locally_edited_old_folder() {
    let dir = TempDir::new().unwrap();
    write_skill(dir.path(), "old-skill", "official v1 plus my notes");
    write_skill(dir.path(), "new-skill", "v2");
    assert!(retire_in(dir.path(), RENAMED).is_empty());
    assert!(dir.path().join("old-skill").exists());
}

#[test]
fn rebinds_only_after_old_folder_is_gone() {
    let dir = TempDir::new().unwrap();
    let conn = bindings_db();
    write_skill(dir.path(), "old-skill", "edited");
    write_skill(dir.path(), "new-skill", "v2");
    assert_eq!(rebind_in(&conn, dir.path(), RENAMED).unwrap(), 0, "old folder still present");

    fs::remove_dir_all(dir.path().join("old-skill")).unwrap();
    assert_eq!(rebind_in(&conn, dir.path(), RENAMED).unwrap(), 2);
    assert_eq!(
        identifiers(&conn),
        vec![
            (1, "agents:new-skill".to_string(), true),
            (2, "agents:new-skill".to_string(), true),
            (3, "agents:other".to_string(), true),
        ]
    );
    assert_eq!(rebind_in(&conn, dir.path(), RENAMED).unwrap(), 0, "idempotent");
}

#[test]
fn official_digests_are_sha256_hex() {
    for skill in RENAMED_OFFICIAL_SKILLS {
        assert!(skill.official_digests.iter().all(|d| d.len() == 64 && d.bytes().all(|b| b.is_ascii_hexdigit())));
        assert_ne!(skill.old, skill.new);
    }
}
