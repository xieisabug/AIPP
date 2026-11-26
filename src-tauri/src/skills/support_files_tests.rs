use super::*;
use tempfile::TempDir;

fn skill_with_files() -> (TempDir, PathBuf) {
    let root = TempDir::new().unwrap();
    let folder = root.path().join("managing-aipp-data");
    fs::create_dir_all(folder.join("references")).unwrap();
    fs::create_dir_all(folder.join("scripts")).unwrap();
    fs::create_dir_all(folder.join(".git")).unwrap();
    fs::write(folder.join("SKILL.md"), "---\nname: x\n---\nbody").unwrap();
    fs::write(folder.join("references/hazards.md"), "# hazards").unwrap();
    fs::write(folder.join("scripts/aipp-db.mjs"), "console.log(1)").unwrap();
    fs::write(folder.join(".aipp-install-source"), "marker").unwrap();
    fs::write(folder.join(".git/config"), "x").unwrap();
    fs::write(root.path().join("secret.txt"), "outside").unwrap();
    (root, folder)
}

#[test]
fn only_folder_based_skill_md_has_a_folder() {
    assert!(skill_folder(Path::new("/a/b/SKILL.md")).is_some());
    assert!(skill_folder(Path::new("/a/b/skill.md")).is_some());
    assert!(skill_folder(Path::new("/a/b/AGENTS.md")).is_none());
}

#[test]
fn reads_files_inside_the_skill_folder() {
    let (_root, folder) = skill_with_files();
    assert_eq!(read_support_file(&folder, "references/hazards.md").unwrap(), "# hazards");
    assert_eq!(read_support_file(&folder, "./references/hazards.md").unwrap(), "# hazards");
}

#[test]
fn rejects_paths_escaping_the_skill_folder() {
    let (root, folder) = skill_with_files();
    for bad in ["../secret.txt", "references/../../secret.txt", "", "references\\hazards.md"] {
        assert!(read_support_file(&folder, bad).is_err(), "{bad} must be rejected");
    }
    let absolute = root.path().join("secret.txt").to_string_lossy().replace('\\', "/");
    assert!(read_support_file(&folder, &absolute).is_err());
    assert!(read_support_file(&folder, "references").is_err(), "directories are not files");
}

#[test]
fn lists_visible_files_without_skill_md() {
    let (_root, folder) = skill_with_files();
    assert_eq!(list_support_files(&folder), vec!["references/hazards.md", "scripts/aipp-db.mjs"]);
}
