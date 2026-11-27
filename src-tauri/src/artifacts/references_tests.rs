use std::collections::BTreeMap;

use super::artifacts_db::ArtifactCollection;
use super::references::{
    collection_files, context_section, entry_file_for, find_references, plan_reference, write_files,
    ArtifactReference, MAX_REFERENCES,
};
use crate::mcp::builtin_mcp::choices::{archive, snapshot::Candidate};
use base64::{engine::general_purpose::STANDARD, Engine};

fn item(id: i64, artifact_type: &str, code: &str, source: Option<&str>) -> ArtifactCollection {
    ArtifactCollection {
        id,
        name: format!("工具{id}"),
        icon: "🧰".into(),
        description: String::new(),
        artifact_type: artifact_type.into(),
        code: code.into(),
        tags: None,
        created_time: String::new(),
        last_used_time: None,
        use_count: 0,
        db_id: None,
        assistant_id: None,
        source: source.map(str::to_owned),
    }
}

fn names(entries: &[(i64, &str)]) -> Vec<(i64, String)> {
    entries.iter().map(|(id, name)| (*id, name.to_string())).collect()
}

#[test]
fn test_find_references_prefers_longest_name_and_dedupes() {
    let names = names(&[(1, "番茄"), (2, "番茄钟"), (3, "Todo App")]);
    assert_eq!(find_references("把 #番茄钟改成 50 分钟，再看看 #todo app 和 #番茄钟", &names), vec![2, 3]);
    assert!(find_references("没有引用 # 番茄钟", &names).is_empty());
}

#[test]
fn test_find_references_caps_the_count() {
    let names = names(&[(1, "a"), (2, "b"), (3, "c"), (4, "d")]);
    assert_eq!(find_references("#a #b #c #d", &names).len(), MAX_REFERENCES);
}

#[test]
fn test_plan_reference_reuses_key_in_source_conversation() {
    let source = r#"{"conversation_id":9,"artifact_key":"ui/timer","version":2}"#;
    assert_eq!(plan_reference(&item(5, "html", "", Some(source)), Some(9), |_| true), ("ui/timer".into(), true, false));
}

#[test]
fn test_plan_reference_links_when_source_missing_or_gone() {
    assert_eq!(plan_reference(&item(5, "html", "", None), Some(1), |_| true), ("collection-5".into(), false, true));
    let source = r#"{"conversation_id":9,"artifact_key":"ui/timer","version":2}"#;
    assert_eq!(plan_reference(&item(5, "html", "", Some(source)), Some(1), |_| false), ("collection-5".into(), false, true));
    // 原会话还在：不抢走来源
    assert_eq!(plan_reference(&item(5, "html", "", Some(source)), Some(1), |_| true), ("collection-5".into(), false, false));
    // 新会话（还没有 ID）
    assert_eq!(plan_reference(&item(5, "html", "", Some(source)), None, |_| true), ("collection-5".into(), false, false));
}

#[test]
fn test_collection_files_names_single_file_by_type() {
    let (entry, files) = collection_files(&item(1, "react", "export default 1", None)).unwrap();
    assert_eq!(entry, "App.tsx");
    assert_eq!(files["App.tsx"], b"export default 1");
    assert_eq!(entry_file_for("markdown"), "README.md");
}

#[test]
fn test_collection_files_unpacks_multi_file_archive() {
    let project = BTreeMap::from([
        ("App.tsx".to_string(), STANDARD.encode("import './a.css'")),
        ("a.css".to_string(), STANDARD.encode("p{}")),
    ]);
    let code = archive::Archive {
        candidate: Candidate {
            id: "artifact".into(),
            title: "t".into(),
            kind: "artifact".into(),
            summary: None,
            language: Some("tsx".into()),
            content: Some("import './a.css'".into()),
            url: None,
            artifact_key: Some("k".into()),
            entry_file: Some("App.tsx".into()),
        },
        files: Some(project),
        build: None,
    }
    .encode()
    .unwrap();
    let (entry, files) = collection_files(&item(1, "html", &code, None)).unwrap();
    assert_eq!(entry, "App.tsx");
    assert_eq!(files.keys().collect::<Vec<_>>(), vec!["App.tsx", "a.css"]);
    assert_eq!(files["a.css"], b"p{}");
}

fn reference(same_conversation: bool, link_source: bool, files: BTreeMap<String, Vec<u8>>) -> ArtifactReference {
    ArtifactReference {
        collection_id: 3,
        name: "番茄钟".into(),
        artifact_type: "html".into(),
        artifact_key: "collection-3".into(),
        entry_file: "index.html".into(),
        files,
        same_conversation,
        link_source,
    }
}

#[test]
fn test_context_section_inlines_entry_first_and_explains_publishing() {
    let files = BTreeMap::from([
        ("a.css".to_string(), b"p{}".to_vec()),
        ("index.html".to_string(), b"<p>hi</p>".to_vec()),
        ("logo.png".to_string(), vec![0xff, 0xfe, 0x00]),
    ]);
    let text = context_section(&reference(false, true, files));
    assert!(text.starts_with("<referenced_artifact name=\"番茄钟\" collection_id=\"3\""));
    assert!(text.contains("artifact_key=\"collection-3\"，entry_file=\"index.html\""));
    assert!(text.contains("用户可在合集里把收藏更新"));
    assert!(text.find("--- index.html ---").unwrap() < text.find("--- a.css ---").unwrap());
    assert!(text.contains("未内联的文件：logo.png"));
    assert!(text.ends_with("</referenced_artifact>"));
}

#[test]
fn test_context_section_skips_code_for_source_conversation() {
    let text = context_section(&reference(true, false, BTreeMap::from([("index.html".to_string(), b"<p>hi</p>".to_vec())])));
    assert!(text.contains("收藏自本会话的 Artifact `collection-3`"));
    assert!(!text.contains("<p>hi</p>"));
}

#[test]
fn test_write_files_rejects_escaping_paths() {
    let dir = std::env::temp_dir().join(format!("aipp-ref-test-{}", std::process::id()));
    let ok = BTreeMap::from([("src/App.tsx".to_string(), b"x".to_vec())]);
    write_files(&dir, &ok).unwrap();
    assert_eq!(std::fs::read(dir.join("src/App.tsx")).unwrap(), b"x");
    let bad = BTreeMap::from([("../evil.txt".to_string(), b"x".to_vec())]);
    assert!(write_files(&dir, &bad).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
