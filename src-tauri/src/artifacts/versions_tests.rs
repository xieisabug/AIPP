use super::*;

fn conn() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "PRAGMA foreign_keys=ON;
         CREATE TABLE conversation (id INTEGER PRIMARY KEY);
         INSERT INTO conversation (id) VALUES (7);",
    )
    .unwrap();
    schema(&conn).unwrap();
    conn
}

fn project(entries: &[(&str, &str)]) -> Project {
    entries.iter().map(|(name, text)| (name.to_string(), STANDARD.encode(text))).collect()
}

fn new_version<'a>(entry_file: &'a str) -> NewVersion<'a> {
    NewVersion {
        conversation_id: 7,
        artifact_key: "ui/card",
        title: "卡片",
        language: "html",
        preview_type: "html",
        entry_file,
        db_id: None,
        assistant_id: None,
    }
}

#[test]
fn identical_content_reuses_the_latest_version() {
    let conn = conn();
    let first = record_version_in(&conn, project(&[("index.html", "<p>a</p>")]), new_version("index.html"), "t1").unwrap();
    assert_eq!(first.version, 1);
    let same = record_version_in(&conn, project(&[("index.html", "<p>a</p>")]), new_version("index.html"), "t2").unwrap();
    assert_eq!(same.version, 1);
    let changed = record_version_in(&conn, project(&[("index.html", "<p>b</p>")]), new_version("index.html"), "t3").unwrap();
    assert_eq!(changed.version, 2);

    let versions = list_versions_in(&conn, 7, "ui/card").unwrap();
    assert_eq!(versions.iter().map(|v| v.version).collect::<Vec<_>>(), vec![2, 1]);
    let old = get_version_in(&conn, 7, "ui/card", Some(1)).unwrap().unwrap();
    assert_eq!(old.entry_code().unwrap(), "<p>a</p>");
    let latest = get_version_in(&conn, 7, "ui/card", None).unwrap().unwrap();
    assert_eq!(latest.summary.version, 2);
    let overview = latest_versions_in(&conn, 7).unwrap();
    assert_eq!(overview["ui/card"], LatestVersion { version: 2, count: 2, multi_file: false });
}

#[test]
fn keeps_only_the_most_recent_versions() {
    let conn = conn();
    for i in 0..(MAX_VERSIONS + 5) {
        record_version_in(&conn, project(&[("index.html", &format!("v{i}"))]), new_version("index.html"), "t").unwrap();
    }
    let versions = list_versions_in(&conn, 7, "ui/card").unwrap();
    assert_eq!(versions.len() as i64, MAX_VERSIONS);
    assert_eq!(versions.last().unwrap().version, 6);
}

#[test]
fn versions_follow_their_conversation() {
    let conn = conn();
    record_version_in(&conn, project(&[("index.html", "x")]), new_version("index.html"), "t").unwrap();
    conn.execute("DELETE FROM conversation WHERE id=7", []).unwrap();
    assert!(list_versions_in(&conn, 7, "ui/card").unwrap().is_empty());
}

#[test]
fn detects_entries_that_load_sibling_files() {
    let files = project(&[
        ("src/App.tsx", ""),
        ("src/Button.tsx", ""),
        ("src/style.css", ""),
        ("index.html", ""),
        ("logo.png", ""),
    ]);
    assert!(references_sibling_files(&files, "src/App.tsx", "import Button from './Button';"));
    assert!(references_sibling_files(&files, "src/App.tsx", "import \"./style.css\";"));
    assert!(references_sibling_files(&files, "index.html", "<script type=module src=\"src/App.tsx\"></script>"));
    assert!(references_sibling_files(&files, "index.html", "<img src='./logo.png'>"));
    assert!(references_sibling_files(&files, "src/App.tsx", "const a = { background: 'url(../logo.png)' }"));

    // 外部地址、npm 包、缺失的文件、自身引用都不算
    assert!(!references_sibling_files(&files, "src/App.tsx", "import React from 'react'; import x from './Missing';"));
    assert!(!references_sibling_files(&files, "index.html", "<script src=\"https://cdn.example.com/a.js\"></script><a href=\"#top\">"));
    assert!(!references_sibling_files(&files, "index.html", "<a href=\"index.html\">self</a>"));
    assert!(!references_sibling_files(&files, "src/App.tsx", "import x from '../../outside';"));
}

#[test]
fn multi_file_flag_is_stored() {
    let conn = conn();
    let files = project(&[("App.tsx", "import B from './B';"), ("B.tsx", "export default 1")]);
    let mut version = new_version("App.tsx");
    version.preview_type = "react";
    let summary = record_version_in(&conn, files, version, "t").unwrap();
    assert!(summary.multi_file);
    assert_eq!(summary.file_count, 2);
}

#[test]
fn build_output_is_stored_per_version() {
    let conn = conn();
    let files = project(&[("index.html", "<script src=\"./main.js\"></script>"), ("main.js", "1"), ("package.json", "{}")]);
    let first = record_version_in(&conn, files, new_version("index.html"), "t1").unwrap();
    assert_eq!(get_build_in(&conn, 7, "ui/card", first.version).unwrap(), None);

    let output = project(&[("index.html", "<p>built</p>")]);
    assert!(set_build_in(&conn, 7, "ui/card", first.version, &output).unwrap());
    let stored: Project = serde_json::from_str(&get_build_in(&conn, 7, "ui/card", first.version).unwrap().unwrap()).unwrap();
    assert_eq!(stored, output);
    assert!(!set_build_in(&conn, 7, "ui/card", 99, &output).unwrap());
}

#[test]
fn schema_adds_build_column_once() {
    let conn = conn();
    schema(&conn).unwrap();
    schema(&conn).unwrap();
}
