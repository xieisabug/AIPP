use std::collections::BTreeMap;

use super::collection_source::{collection_type, find_updates, CollectionSourceUpdate};
use super::versions::LatestVersion;

fn source(conversation_id: i64, key: &str, version: i64) -> String {
    format!(r#"{{"conversation_id":{conversation_id},"artifact_key":"{key}","version":{version}}}"#)
}

fn latest(entries: &[(&str, i64)]) -> BTreeMap<String, LatestVersion> {
    entries
        .iter()
        .map(|(key, version)| (key.to_string(), LatestVersion { version: *version, count: *version, multi_file: false }))
        .collect()
}

#[test]
fn test_find_updates_reports_only_newer_versions() {
    let sources = vec![(1, source(10, "a", 1)), (2, source(10, "b", 3)), (3, source(10, "missing", 1))];
    let updates = find_updates(&sources, |_| Some(latest(&[("a", 4), ("b", 3)])));
    assert_eq!(updates, vec![CollectionSourceUpdate { id: 1, current_version: 1, latest_version: 4 }]);
}

#[test]
fn test_find_updates_reads_each_conversation_once() {
    let sources = vec![(1, source(10, "a", 1)), (2, source(10, "b", 1)), (3, source(11, "a", 1))];
    let mut reads = Vec::new();
    let updates = find_updates(&sources, |conversation_id| {
        reads.push(conversation_id);
        Some(latest(&[("a", 2), ("b", 2)]))
    });
    assert_eq!(reads, vec![10, 11]);
    assert_eq!(updates.len(), 3);
}

#[test]
fn test_find_updates_skips_deleted_conversations_and_bad_sources() {
    let sources = vec![(1, source(10, "a", 1)), (2, "not json".to_string())];
    assert!(find_updates(&sources, |_| None).is_empty());
}

#[test]
fn test_collection_type_maps_preview_types_with_fallback() {
    assert_eq!(collection_type("tsx", "html"), "react");
    assert_eq!(collection_type("markdown", "html"), "markdown");
    assert_eq!(collection_type("drawio", "html"), "xml");
    assert_eq!(collection_type("Vue", "html"), "vue");
    assert_eq!(collection_type("powershell", "react"), "react");
}
