use super::*;

#[test]
fn test_onboarding_setup_manual_skills_require_all_sources_and_probe_never_writes_local_files() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    let target = temp.path().join("destination");
    let source = SkillInstallRecipeSource {
        source_type: SkillInstallRecipeSourceType::GitHub,
        repo: Some("official/example".into()), git_ref: "main".into(), url: None,
    };
    for name in ["alpha", "beta"] {
        let dir = repo.join("skills").join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("SKILL.md"), format!("# {name}\n\nDescription")).unwrap();
    }
    // An empty configured directory list means discovery of the whole package.
    let entries = archive_manifest(&source, Some(&[]), &repo, &target).unwrap();
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().all(|e| !e.installed_at(&target).unwrap()));
    assert!(!target.exists());

    let alpha = target.join("alpha");
    fs::create_dir_all(&alpha).unwrap();
    fs::copy(repo.join("skills/alpha/SKILL.md"), alpha.join("SKILL.md")).unwrap();
    assert!(entries[0].installed_at(&target).unwrap());
    assert!(!entries[1].installed_at(&target).unwrap());
    assert!(!official_skills_ready(&entries, &target).unwrap());
    assert!(!alpha.join(".aipp-install-source").exists());
    let beta = target.join("beta");
    fs::create_dir_all(&beta).unwrap();
    fs::copy(repo.join("skills/beta/SKILL.md"), beta.join("SKILL.md")).unwrap();
    assert!(official_skills_ready(&entries, &target).unwrap());
    fs::write(alpha.join("SKILL.md"), "同名的其他技能").unwrap();
    assert!(entries[0].installed_at(&target).unwrap_err().contains("其他来源"));
    assert_eq!(fs::read_to_string(alpha.join("SKILL.md")).unwrap(), "同名的其他技能");
    fs::write(beta.join(".aipp-install-source"), "different/source").unwrap();
    assert!(entries[1].installed_at(&target).unwrap_err().contains("其他来源"));
    fs::write(beta.join(".aipp-install-source"), &entries[1].source_id).unwrap();
    fs::write(beta.join("SKILL.md"), "用户编辑的技能").unwrap();
    assert!(entries[1].installed_at(&target).unwrap());
    fs::remove_file(beta.join("SKILL.md")).unwrap();
    assert!(entries[1].installed_at(&target).unwrap_err().contains("缺少入口"));
    fs::remove_file(alpha.join("SKILL.md")).unwrap();
    fs::remove_dir(&alpha).unwrap();
    assert!(!entries[0].installed_at(&target).unwrap(), "cached manifest must observe an uninstall");
    assert!(official_skills_ready(&entries, &target).unwrap_err().contains("beta"), "a missing member must not hide a conflict");
    assert!(!alpha.exists());

    let mapped = [SkillInstallRecipeDir { from: "skills/beta".into(), to: "selected".into() }];
    let selected = archive_manifest(&source, Some(&mapped), &repo, &target).unwrap();
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].target, "selected");
}
