use super::format::*;
use serde_json::{json, Value};

fn source() -> Source { Source { repo: "openai/plugins".into(), git_ref: "main".into(), path: "plugins/notion".into() } }
fn files() -> Files {
    Files::from([
        (".codex-plugin/plugin.json".into(), serde_json::to_vec(&json!({"name":"notion","version":"1.0","skills":"./skills/","mcpServers":"./.mcp.json"})).unwrap()),
        (".mcp.json".into(), serde_json::to_vec(&json!({"mcpServers":{"notion":{"type":"http","url":"https://mcp.notion.com/mcp","oauth_resource":"https://mcp.notion.com"}}})).unwrap()),
        ("skills/capture/SKILL.md".into(), b"---\nname: capture\ndescription: >-\n  Capture notes\n  into Notion.\nmetadata:\n  version: '1.2'\n---\nRead reference/template.md".to_vec()),
        ("skills/capture/reference/template.md".into(), b"Template".to_vec()),
    ])
}

#[test]
fn test_agent_plugin_preserves_components_and_nested_yaml() {
    let preview = inspect(source(), "a".repeat(40), &files()).unwrap();
    assert_eq!(preview.skills.len(), 1);
    assert_eq!(preview.skills[0].description, "Capture notes into Notion.");
    assert_eq!(preview.skills[0].files, vec!["skills/capture/reference/template.md"]);
    assert!(preview.servers[0].reason.is_none());
    assert_eq!(preview.servers[0].config["oauth_resource"], "https://mcp.notion.com");
}

#[test]
fn test_agent_plugin_reads_public_upstream_packages() {
    let fixtures: Vec<Value> = serde_json::from_str(include_str!("../../../../tests/fixtures/agent-plugins/upstream.json")).unwrap();
    assert_eq!(fixtures.len(), 62);
    for fixture in fixtures {
        let source: Source = serde_json::from_value(fixture["source"].clone()).unwrap();
        let files: Files = fixture["files"].as_object().unwrap().iter().map(|(p,v)| (p.clone(),v.as_str().unwrap().as_bytes().to_vec())).collect();
        let result = inspect(source.clone(), "d420d13051bffe4543ed39e30fa46b87148e47dd".into(), &files);
        assert!(result.is_ok(), "{}: {:?}", source.path, result.err());
    }
}

#[test]
fn test_agent_plugin_rejects_unsafe_paths_and_sources() {
    for path in ["../bad", "/bad", "foo\\bar", "foo:bar", "foo/../bar", "CON", "skills/test.", "foo//bar"] { assert!(safe_path(path).is_err(), "{path}"); }
    let mut invalid_source = source(); invalid_source.repo = "owner/repo/extra".into(); assert!(invalid_source.validate().is_err());
    let mut value = files(); value.insert(".codex-plugin/plugin.json".into(), br#"{"name":"bad","skills":"../outside"}"#.to_vec());
    assert!(inspect(source(), "a".repeat(40), &value).is_err());
}

#[test]
fn test_agent_plugin_reports_host_only_dependencies() {
    let mut value = files();
    value.insert(".app.json".into(), br#"{"apps":{"private-service":{"id":"asdk_private"}}}"#.to_vec());
    value.insert(".mcp.json".into(), br#"{"mcpServers":{"gmail":{"type":"http","url":"https://gmailmcp.googleapis.com/mcp/v1","oauth":{"client_id":"<CLIENT_ID>"}}}}"#.to_vec());
    let p = inspect(source(), "a".repeat(40), &value).unwrap();
    assert!(p.notices.iter().any(|n| n.contains("没有独立 MCP")));
    assert!(p.servers[0].reason.as_ref().unwrap().contains("OAuth"));
}

#[test]
fn test_agent_plugin_digest_covers_supporting_files() {
    let mut value = files(); let before = digest(&value);
    value.insert("skills/capture/reference/template.md".into(), b"changed".to_vec());
    assert_ne!(before, digest(&value));
    assert_eq!(digest(&value), digest(&value.clone()));
    let mut preview = inspect(source(), "a".repeat(40), &value).unwrap();
    preview.set_permissions(std::collections::BTreeMap::from([("script.sh".into(), 0o644)]));
    let before = preview.digest.clone();
    preview.set_permissions(std::collections::BTreeMap::from([("script.sh".into(), 0o755)]));
    assert_ne!(before, preview.digest);
}

#[test]
fn test_agent_plugin_unpacks_only_selected_package() {
    use std::io::{Cursor, Write};
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    for (name, bytes) in files() { writer.start_file(format!("repo-sha/plugins/notion/{name}"), options).unwrap(); writer.write_all(&bytes).unwrap(); }
    writer.start_file("repo-sha/plugins/other/secret.txt", options).unwrap(); writer.write_all(b"unrelated").unwrap();
    let archive = writer.finish().unwrap().into_inner();
    assert_eq!(unpack(&archive, &source()).unwrap(), files());
}

#[test]
fn test_agent_plugin_portable_manifest_has_distinct_identity() {
    let mut value = files();
    value.insert("plugin.json".into(), br#"{"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":"portable"}"#.to_vec());
    value.insert("mcp.json".into(), value[".mcp.json"].clone());
    assert_eq!(inspect(source(), "a".repeat(40), &value).unwrap().name, "portable");
    value.insert("plugin.json".into(), br#"{"name":"legacy-aipp","code":"legacy"}"#.to_vec());
    assert!(inspect(source(), "a".repeat(40), &value).is_err());
}

#[test]
fn test_agent_plugin_install_update_and_rollback_preserve_package() {
    // Isolated package files only; this test never opens a file-backed database.
    let directory = tempfile::tempdir().unwrap();
    let first = files(); let initial = inspect(source(), "a".repeat(40), &first).unwrap();
    super::store::install(directory.path(), &initial, &first).unwrap();
    let mut second = first.clone(); second.insert("skills/capture/reference/template.md".into(), b"new template".to_vec());
    let update = inspect(source(), "b".repeat(40), &second).unwrap();
    super::store::install(directory.path(), &update, &second).unwrap();
    super::store::install(directory.path(), &update, &second).unwrap(); // Retry must retain the old backup.
    assert_eq!(super::store::read_skill_file(directory.path(), &update.skills[0].id, "reference/template.md").unwrap(), "new template");
    super::store::rollback(directory.path(), &initial.key).unwrap();
    assert_eq!(super::store::receipt(directory.path(), &initial.key).unwrap().unwrap().preview.revision, initial.revision);
    assert_eq!(super::store::read_skill_file(directory.path(), &initial.skills[0].id, "reference/template.md").unwrap(), "Template");
}

#[test]
fn test_agent_plugin_modified_files_prevent_update_and_uninstall() {
    let directory = tempfile::tempdir().unwrap();
    let first = files(); let initial = inspect(source(), "a".repeat(40), &first).unwrap();
    super::store::install(directory.path(), &initial, &first).unwrap();
    let edited = super::store::active(directory.path(), &initial.key).unwrap().join("skills/capture/reference/template.md");
    std::fs::write(&edited, "user edit").unwrap();
    assert!(super::store::install(directory.path(), &initial, &first).unwrap_err().contains("本地修改"));
    assert!(super::store::uninstall(directory.path(), &initial.key).unwrap_err().contains("本地修改"));
    assert_eq!(std::fs::read_to_string(edited).unwrap(), "user edit");
}

#[test]
fn test_agent_plugin_interrupted_commit_recovers_previous_revision() {
    let directory = tempfile::tempdir().unwrap();
    let first = files(); let initial = inspect(source(), "a".repeat(40), &first).unwrap();
    super::store::install(directory.path(), &initial, &first).unwrap();
    let current = super::store::active(directory.path(), &initial.key).unwrap();
    std::fs::rename(&current, current.parent().unwrap().join("previous")).unwrap();
    assert_eq!(super::store::scan_installed_skills(directory.path()).len(), 1);
    let mut next = first.clone(); next.insert("new.txt".into(), b"new".to_vec());
    let update = inspect(source(), "b".repeat(40), &next).unwrap();
    super::store::install(directory.path(), &update, &next).unwrap();
    assert_eq!(super::store::receipt(directory.path(), &update.key).unwrap().unwrap().preview.revision, update.revision);
}

#[test]
fn test_agent_plugin_supporting_file_cannot_escape_package() {
    let directory = tempfile::tempdir().unwrap();
    let first = files(); let initial = inspect(source(), "a".repeat(40), &first).unwrap();
    super::store::install(directory.path(), &initial, &first).unwrap();
    for path in ["../../../outside", "C:/secrets", "reference/../../outside"] {
        assert!(super::store::read_skill_file(directory.path(), &initial.skills[0].id, path).is_err());
    }
}

#[test]
fn test_agent_plugin_required_files_use_package_boundary() {
    let directory = tempfile::tempdir().unwrap();
    let mut value = files();
    value.insert("skills/capture/SKILL.md".into(), b"---\nname: capture\nrequires_files: [reference/template.md]\n---\nBody".to_vec());
    let preview = inspect(source(), "a".repeat(40), &value).unwrap();
    super::store::install(directory.path(), &preview, &value).unwrap();
    let content = super::store::read_skill_content(directory.path(), &preview.skills[0].id).unwrap();
    assert_eq!(content.additional_files[0].content, "Template");
    assert!(content.content.contains("\nreference/template.md\n"));
    let skill = super::store::active(directory.path(), &preview.key).unwrap().join("skills/capture/SKILL.md");
    std::fs::write(skill, "---\nname: capture\nrequires_files: ['../../../outside']\n---\nBody").unwrap();
    assert!(super::store::read_skill_content(directory.path(), &preview.skills[0].id).is_err());
}

#[test]
fn test_agent_plugin_commands_and_agent_definitions_are_runnable_content() {
    let directory=tempfile::tempdir().unwrap();let mut value=files();
    value.insert("commands/review.md".into(),b"# Review\nRead ../skills/capture/reference/template.md and review $ARGUMENTS".to_vec());
    value.insert("agents/reviewer.md".into(),b"You are the reviewer. Report evidence.".to_vec());
    let preview=inspect(source(),"a".repeat(40),&value).unwrap();
    assert_eq!(preview.agents.len(),1);
    let command=preview.skills.iter().find(|s|s.kind=="command").unwrap();
    super::store::install(directory.path(),&preview,&value).unwrap();
    assert!(super::store::read_skill_content(directory.path(),&command.id).unwrap().content.contains("$ARGUMENTS"));
    assert_eq!(super::store::read_skill_file(directory.path(),&command.id,"../skills/capture/reference/template.md").unwrap(),"Template");
    assert!(super::store::read_skill_file(directory.path(),&command.id,"../../outside").is_err());
}

#[test]
#[ignore = "Requires the pinned upstream ZIP through AIPP_PLUGIN_TEST_ARCHIVE; no network or account access"]
fn test_agent_plugin_full_upstream_archive() {
    let path = std::env::var("AIPP_PLUGIN_TEST_ARCHIVE").unwrap();
    let archive = std::fs::read(path).unwrap();
    let fixtures: Vec<Value> = serde_json::from_str(include_str!("../../../../tests/fixtures/agent-plugins/upstream.json")).unwrap();
    for fixture in fixtures {
        let source: Source = serde_json::from_value(fixture["source"].clone()).unwrap();
        let files = unpack(&archive, &source).unwrap();
        let preview = inspect(source.clone(), "d420d13051bffe4543ed39e30fa46b87148e47dd".into(), &files).unwrap();
        if source.path == "plugins/notion" {
            assert_eq!(preview.skills.len(), 4);
            assert!(preview.skills.iter().all(|s| !s.files.is_empty()));
            let directory = tempfile::tempdir().unwrap();
            super::store::install(directory.path(), &preview, &files).unwrap();
            for skill in &preview.skills { assert!(super::store::read_skill_content(directory.path(), &skill.id).is_ok()); }
        }
    }
}

// Maintainer-only exporter: publish parsed metadata with the helper release.
#[test]
#[ignore = "Exports pinned metadata for AIPP-helper; requires archive and output paths"]
fn test_export_helper_plugin_catalog() {
    fn redact(value: &mut Value) {
        match value {
            Value::Object(map) => { map.remove("client_secret"); for child in map.values_mut() { redact(child); } }
            Value::Array(items) => for child in items { redact(child); },
            _ => {},
        }
    }
    let archive = std::fs::read(std::env::var("AIPP_PLUGIN_TEST_ARCHIVE").unwrap()).unwrap();
    let fixtures: Vec<Value> = serde_json::from_str(include_str!("../../../../tests/fixtures/agent-plugins/upstream.json")).unwrap();
    let mut previews = Vec::new();
    for fixture in fixtures {
        let source: Source = serde_json::from_value(fixture["source"].clone()).unwrap();
        let files = unpack(&archive, &source).unwrap();
        let mut preview = inspect(source.clone(), "d420d13051bffe4543ed39e30fa46b87148e47dd".into(), &files).unwrap();
        preview.set_permissions(permissions(&archive, &source).unwrap());
        super::dependencies::resolve(&mut preview, &archive).unwrap();
        let mut value = serde_json::to_value(preview).unwrap(); redact(&mut value); previews.push(value);
    }
    std::fs::write(std::env::var("AIPP_PLUGIN_EXPORT").unwrap(), serde_json::to_vec(&previews).unwrap()).unwrap();
}

#[test]
#[ignore = "Verifies a helper single-plugin artifact against its published preview"]
fn test_helper_single_plugin_archive_matches_preview() {
    let archive = std::fs::read(std::env::var("AIPP_PLUGIN_TEST_ARCHIVE").unwrap()).unwrap();
    let expected: super::format::Preview = serde_json::from_slice(&std::fs::read(std::env::var("AIPP_PLUGIN_PREVIEW").unwrap()).unwrap()).unwrap();
    let files = unpack(&archive, &expected.source).unwrap();
    let mut actual = inspect(expected.source.clone(), expected.revision.clone(), &files).unwrap();
    actual.set_permissions(permissions(&archive, &expected.source).unwrap());
    super::dependencies::resolve(&mut actual, &archive).unwrap();
    assert_eq!(actual.digest, expected.digest);
    assert_eq!(actual.files, expected.files);
    assert_eq!(actual.skills.len(), expected.skills.len());
    assert_eq!(serde_json::to_value(&actual.dependencies).unwrap(), serde_json::to_value(&expected.dependencies).unwrap());
    let directory = tempfile::tempdir().unwrap();
    super::store::install(directory.path(), &actual, &files).unwrap();
    for skill in &actual.skills {
        assert!(super::store::read_skill_content(directory.path(), &skill.id).is_ok());
    }
}
