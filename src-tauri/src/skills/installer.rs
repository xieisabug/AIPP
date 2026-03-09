use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Cursor;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillInstallRecipe {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub source: SkillInstallRecipeSource,
    pub dirs: Vec<SkillInstallRecipeDir>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillInstallRecipeSource {
    #[serde(rename = "type")]
    pub source_type: SkillInstallRecipeSourceType,
    pub repo: String,
    #[serde(rename = "ref", default = "default_git_ref")]
    pub git_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillInstallRecipeSourceType {
    GitHub,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillInstallRecipeDir {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillInstallPlanSkill {
    pub from: String,
    pub to: String,
    pub detected_entry_file: String,
    pub normalized_entry_file: String,
    pub will_replace: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillInstallPlan {
    pub recipe_id: String,
    pub recipe_name: String,
    pub source_repo: String,
    pub source_ref: String,
    pub download_url: String,
    pub target_directory: String,
    pub skills: Vec<SkillInstallPlanSkill>,
    pub steps: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillInstallResult {
    pub recipe_id: String,
    pub recipe_name: String,
    pub source_repo: String,
    pub source_ref: String,
    pub target_directory: String,
    pub installed_skills: Vec<SkillInstallPlanSkill>,
}

fn default_git_ref() -> String {
    "main".to_string()
}

impl SkillInstallRecipe {
    pub fn validate(&self) -> Result<(), String> {
        if self.id.trim().is_empty() {
            return Err("Recipe id cannot be empty".to_string());
        }
        if self.name.trim().is_empty() {
            return Err("Recipe name cannot be empty".to_string());
        }
        self.source.validate()?;

        if self.dirs.is_empty() {
            return Err("Recipe must contain at least one directory mapping".to_string());
        }

        let mut seen_targets = std::collections::HashSet::new();
        for dir in &self.dirs {
            validate_relative_source_path(&dir.from)?;
            validate_target_dir_name(&dir.to)?;

            if !seen_targets.insert(dir.to.clone()) {
                return Err(format!("Duplicate target directory in recipe: {}", dir.to));
            }
        }

        Ok(())
    }
}

impl SkillInstallRecipeSource {
    fn validate(&self) -> Result<(), String> {
        match self.source_type {
            SkillInstallRecipeSourceType::GitHub => validate_github_repo(&self.repo)?,
        }

        if self.git_ref.trim().is_empty() {
            return Err("Git ref cannot be empty".to_string());
        }

        Ok(())
    }

    fn archive_url(&self) -> String {
        match self.source_type {
            SkillInstallRecipeSourceType::GitHub => {
                format!("https://codeload.github.com/{}/zip/{}", self.repo, self.git_ref)
            }
        }
    }
}

pub fn parse_skill_install_recipe(recipe_contents: &str) -> Result<SkillInstallRecipe, String> {
    let recipe = serde_json::from_str::<SkillInstallRecipe>(recipe_contents)
        .map_err(|e| format!("Failed to parse skill install recipe JSON: {}", e))?;
    recipe.validate()?;
    Ok(recipe)
}

pub fn load_skill_install_recipe_from_file(
    recipe_path: &Path,
) -> Result<SkillInstallRecipe, String> {
    let recipe_contents = fs::read_to_string(recipe_path).map_err(|e| {
        format!("Failed to read skill install recipe file {}: {}", recipe_path.display(), e)
    })?;
    parse_skill_install_recipe(&recipe_contents)
}

pub async fn inspect_skill_install_recipe(
    recipe: &SkillInstallRecipe,
    skills_dir: &Path,
) -> Result<SkillInstallPlan, String> {
    let archive = download_and_extract_recipe(recipe).await?;
    build_install_plan(recipe, &archive.repo_root, skills_dir, &archive.download_url)
}

pub async fn install_skill_install_recipe(
    recipe: &SkillInstallRecipe,
    skills_dir: &Path,
) -> Result<SkillInstallResult, String> {
    let archive = download_and_extract_recipe(recipe).await?;
    install_recipe_from_repo_root(recipe, &archive.repo_root, skills_dir, &archive.download_url)
}

pub(crate) fn extract_zip_bytes_to_dir(bytes: &[u8], output_dir: &Path) -> Result<(), String> {
    let cursor = Cursor::new(bytes);
    let mut zip =
        zip::ZipArchive::new(cursor).map_err(|e| format!("Failed to open zip archive: {}", e))?;

    for i in 0..zip.len() {
        let mut file =
            zip.by_index(i).map_err(|e| format!("Failed to get file from zip: {}", e))?;
        let file_path = output_dir.join(file.mangled_name());

        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("Failed to create directory: {}", e))?;
        }

        if file.name().ends_with('/') {
            continue;
        }

        let mut output =
            fs::File::create(&file_path).map_err(|e| format!("Failed to create file: {}", e))?;
        std::io::copy(&mut file, &mut output)
            .map_err(|e| format!("Failed to write file: {}", e))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            if let Some(mode) = file.unix_mode() {
                let mut perms = fs::Permissions::from_mode(0o644);
                perms.set_mode(mode);
                output
                    .set_permissions(perms)
                    .map_err(|e| format!("Failed to set permissions: {}", e))?;
            }
        }
    }

    Ok(())
}

pub(crate) fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| format!("Failed to create directory: {}", e))?;

    for entry in fs::read_dir(src).map_err(|e| format!("Failed to read directory: {}", e))? {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path).map_err(|e| format!("Failed to copy file: {}", e))?;
        }
    }

    Ok(())
}

struct TempExtractDir {
    path: PathBuf,
}

impl TempExtractDir {
    fn new(prefix: &str) -> Result<Self, String> {
        let path = std::env::temp_dir().join(format!("{}_{}", prefix, uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).map_err(|e| {
            format!("Failed to create temporary extraction directory {}: {}", path.display(), e)
        })?;
        Ok(Self { path })
    }
}

impl Drop for TempExtractDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

struct DownloadedRecipeArchive {
    _temp_dir: TempExtractDir,
    repo_root: PathBuf,
    download_url: String,
}

async fn download_and_extract_recipe(
    recipe: &SkillInstallRecipe,
) -> Result<DownloadedRecipeArchive, String> {
    recipe.validate()?;

    let download_url = recipe.source.archive_url();
    let client = reqwest::Client::builder()
        .user_agent("AIPP skill installer")
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))?;

    let response = client
        .get(&download_url)
        .send()
        .await
        .map_err(|e| format!("Failed to download GitHub archive: {}", e))?;

    if !response.status().is_success() {
        return Err(format!(
            "Failed to download GitHub archive {}: {}",
            download_url,
            response.status()
        ));
    }

    let archive_bytes =
        response.bytes().await.map_err(|e| format!("Failed to read downloaded archive: {}", e))?;

    let temp_dir = TempExtractDir::new("skill_recipe_extract")?;
    extract_zip_bytes_to_dir(archive_bytes.as_ref(), &temp_dir.path)?;
    let repo_root = resolve_archive_root(&temp_dir.path)?;

    Ok(DownloadedRecipeArchive { _temp_dir: temp_dir, repo_root, download_url })
}

fn build_install_plan(
    recipe: &SkillInstallRecipe,
    repo_root: &Path,
    skills_dir: &Path,
    download_url: &str,
) -> Result<SkillInstallPlan, String> {
    recipe.validate()?;

    let mut skills = Vec::with_capacity(recipe.dirs.len());
    for dir in &recipe.dirs {
        let source_dir = resolve_recipe_dir(repo_root, &dir.from)?;
        let entry_file = find_skill_entry_file_case_insensitive(&source_dir)?.ok_or_else(|| {
            format!(
                "Directory {} does not contain a recognizable skill entry markdown file",
                dir.from
            )
        })?;

        let detected_entry_file = entry_file
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| format!("Invalid UTF-8 file name in {}", entry_file.display()))?
            .to_string();

        let normalized_entry_file = normalized_entry_file_name(&entry_file)?;

        skills.push(SkillInstallPlanSkill {
            from: dir.from.clone(),
            to: dir.to.clone(),
            detected_entry_file,
            normalized_entry_file,
            will_replace: skills_dir.join(&dir.to).exists(),
        });
    }

    let mut steps = vec![
        format!("Download GitHub archive: {}", download_url),
        "Extract archive to a temporary directory".to_string(),
        "Resolve the repository root inside the extracted archive".to_string(),
    ];

    for skill in &skills {
        let target_path = skills_dir.join(&skill.to);
        steps.push(if skill.will_replace {
            format!("Replace {} with {}", skill.from, target_path.display())
        } else {
            format!("Copy {} to {}", skill.from, target_path.display())
        });

        if skill.detected_entry_file != skill.normalized_entry_file {
            steps.push(format!(
                "Normalize entry file {} to {} inside {}",
                skill.detected_entry_file, skill.normalized_entry_file, skill.to
            ));
        }
    }

    steps.push("Refresh installed skills with scan_skills".to_string());

    Ok(SkillInstallPlan {
        recipe_id: recipe.id.clone(),
        recipe_name: recipe.name.clone(),
        source_repo: recipe.source.repo.clone(),
        source_ref: recipe.source.git_ref.clone(),
        download_url: download_url.to_string(),
        target_directory: skills_dir.to_string_lossy().to_string(),
        skills,
        steps,
    })
}

fn install_recipe_from_repo_root(
    recipe: &SkillInstallRecipe,
    repo_root: &Path,
    skills_dir: &Path,
    download_url: &str,
) -> Result<SkillInstallResult, String> {
    let plan = build_install_plan(recipe, repo_root, skills_dir, download_url)?;
    fs::create_dir_all(skills_dir).map_err(|e| {
        format!("Failed to create skills directory {}: {}", skills_dir.display(), e)
    })?;

    for skill in &plan.skills {
        let source_dir = resolve_recipe_dir(repo_root, &skill.from)?;
        let target_dir = skills_dir.join(&skill.to);

        replace_existing_target(&target_dir)?;
        copy_dir_recursive(&source_dir, &target_dir)?;
        normalize_selected_entry_file(
            &target_dir,
            &skill.detected_entry_file,
            &skill.normalized_entry_file,
        )?;

        if find_skill_entry_file_for_current_scanner(&target_dir)?.is_none() {
            return Err(format!(
                "Installed skill {} but AIPP scanner still cannot detect an entry markdown file",
                skill.to
            ));
        }
    }

    Ok(SkillInstallResult {
        recipe_id: plan.recipe_id,
        recipe_name: plan.recipe_name,
        source_repo: plan.source_repo,
        source_ref: plan.source_ref,
        target_directory: plan.target_directory,
        installed_skills: plan.skills,
    })
}

fn validate_github_repo(repo: &str) -> Result<(), String> {
    let mut segments = repo.split('/');
    let owner = segments.next().unwrap_or_default().trim();
    let name = segments.next().unwrap_or_default().trim();

    if owner.is_empty() || name.is_empty() || segments.next().is_some() {
        return Err(format!("GitHub repo must be in owner/repo format, got: {}", repo));
    }

    Ok(())
}

fn validate_relative_source_path(path: &str) -> Result<(), String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("Recipe source directory cannot be empty".to_string());
    }

    for component in Path::new(trimmed).components() {
        match component {
            Component::Normal(_) => {}
            _ => {
                return Err(format!("Invalid recipe source directory path: {}", path));
            }
        }
    }

    Ok(())
}

fn validate_target_dir_name(path: &str) -> Result<(), String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("Recipe target directory cannot be empty".to_string());
    }

    let mut components = Path::new(trimmed).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Ok(()),
        _ => Err(format!("Recipe target directory must be a single directory name, got: {}", path)),
    }
}

fn resolve_archive_root(extract_dir: &Path) -> Result<PathBuf, String> {
    let entries = collect_directory_entries(extract_dir)?;
    if entries.is_empty() {
        return Err(format!("Extracted archive directory {} is empty", extract_dir.display()));
    }

    let has_files = entries.iter().any(|path| path.is_file());
    let dirs: Vec<PathBuf> = entries.into_iter().filter(|path| path.is_dir()).collect();

    if dirs.len() == 1 && !has_files {
        Ok(dirs[0].clone())
    } else {
        Ok(extract_dir.to_path_buf())
    }
}

fn resolve_recipe_dir(repo_root: &Path, from: &str) -> Result<PathBuf, String> {
    validate_relative_source_path(from)?;

    let resolved = repo_root.join(from);
    if !resolved.exists() {
        return Err(format!(
            "Configured skill directory {} does not exist in extracted repository",
            from
        ));
    }
    if !resolved.is_dir() {
        return Err(format!(
            "Configured skill directory {} is not a directory in the extracted repository",
            from
        ));
    }

    let canonical_root = repo_root
        .canonicalize()
        .map_err(|e| format!("Failed to resolve repository root {}: {}", repo_root.display(), e))?;
    let canonical_resolved = resolved.canonicalize().map_err(|e| {
        format!("Failed to resolve configured directory {}: {}", resolved.display(), e)
    })?;

    if !canonical_resolved.starts_with(&canonical_root) {
        return Err(format!("Configured directory {} resolves outside of the archive root", from));
    }

    Ok(resolved)
}

fn find_skill_entry_file_case_insensitive(skill_dir: &Path) -> Result<Option<PathBuf>, String> {
    let entries = collect_directory_entries(skill_dir)?;
    let files: Vec<PathBuf> = entries.into_iter().filter(|path| path.is_file()).collect();

    if let Some(path) = files.iter().find(|path| file_name_eq(path, "SKILL.md")) {
        return Ok(Some(path.clone()));
    }

    if let Some(path) = files.iter().find(|path| file_name_eq(path, "README.md")) {
        return Ok(Some(path.clone()));
    }

    Ok(files.into_iter().find(|path| has_markdown_extension(path)))
}

fn find_skill_entry_file_for_current_scanner(skill_dir: &Path) -> Result<Option<PathBuf>, String> {
    let entries = collect_directory_entries(skill_dir)?;
    let files: Vec<PathBuf> = entries.into_iter().filter(|path| path.is_file()).collect();

    if let Some(path) = files
        .iter()
        .find(|path| path.file_name().and_then(|name| name.to_str()) == Some("SKILL.md"))
    {
        return Ok(Some(path.clone()));
    }

    if let Some(path) = files
        .iter()
        .find(|path| path.file_name().and_then(|name| name.to_str()) == Some("README.md"))
    {
        return Ok(Some(path.clone()));
    }

    Ok(files.into_iter().find(|path| path.extension().and_then(|ext| ext.to_str()) == Some("md")))
}

fn normalized_entry_file_name(entry_path: &Path) -> Result<String, String> {
    let file_name = entry_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("Invalid UTF-8 file name in {}", entry_path.display()))?;

    if file_name.eq_ignore_ascii_case("SKILL.md") {
        return Ok("SKILL.md".to_string());
    }

    if file_name.eq_ignore_ascii_case("README.md") {
        return Ok("README.md".to_string());
    }

    let stem = entry_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| format!("Invalid UTF-8 file stem in {}", entry_path.display()))?;

    Ok(format!("{}.md", stem))
}

fn normalize_selected_entry_file(
    installed_skill_dir: &Path,
    detected_entry_file: &str,
    normalized_entry_file: &str,
) -> Result<(), String> {
    if detected_entry_file == normalized_entry_file {
        return Ok(());
    }

    let source_path = installed_skill_dir.join(detected_entry_file);
    let target_path = installed_skill_dir.join(normalized_entry_file);

    if !source_path.exists() {
        return Err(format!(
            "Expected installed entry file {} to exist in {}",
            detected_entry_file,
            installed_skill_dir.display()
        ));
    }

    if contains_exact_file_name(installed_skill_dir, normalized_entry_file)? {
        return Err(format!(
            "Cannot normalize {} to {} in {} because the target file already exists",
            detected_entry_file,
            normalized_entry_file,
            installed_skill_dir.display()
        ));
    }

    rename_with_case_support(&source_path, &target_path)
}

fn rename_with_case_support(source_path: &Path, target_path: &Path) -> Result<(), String> {
    match fs::rename(source_path, target_path) {
        Ok(()) => Ok(()),
        Err(rename_error) => {
            let temp_path =
                target_path.with_file_name(format!(".aipp-rename-{}", uuid::Uuid::new_v4()));

            fs::rename(source_path, &temp_path).map_err(|e| {
                format!(
                    "Failed to rename {} to temporary file while normalizing to {}: {} (original error: {})",
                    source_path.display(),
                    target_path.display(),
                    e,
                    rename_error
                )
            })?;

            fs::rename(&temp_path, target_path).map_err(|e| {
                let _ = fs::rename(&temp_path, source_path);
                format!(
                    "Failed to rename temporary file {} to {}: {}",
                    temp_path.display(),
                    target_path.display(),
                    e
                )
            })
        }
    }
}

fn replace_existing_target(target_dir: &Path) -> Result<(), String> {
    if !target_dir.exists() {
        return Ok(());
    }

    if target_dir.is_dir() {
        fs::remove_dir_all(target_dir).map_err(|e| {
            format!("Failed to replace existing skill {}: {}", target_dir.display(), e)
        })
    } else {
        fs::remove_file(target_dir).map_err(|e| {
            format!("Failed to remove file {} before install: {}", target_dir.display(), e)
        })
    }
}

fn collect_directory_entries(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = fs::read_dir(dir)
        .map_err(|e| format!("Failed to read directory {}: {}", dir.display(), e))?;

    entries
        .map(|entry| entry.map(|value| value.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to read directory entry in {}: {}", dir.display(), e))
}

fn contains_exact_file_name(dir: &Path, expected: &str) -> Result<bool, String> {
    Ok(collect_directory_entries(dir)?
        .into_iter()
        .any(|path| path.file_name().and_then(|name| name.to_str()) == Some(expected)))
}

fn file_name_eq(path: &Path, expected: &str) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.eq_ignore_ascii_case(expected))
        .unwrap_or(false)
}

fn has_markdown_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("md"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn create_recipe(repo: &str, from: &str, to: &str) -> SkillInstallRecipe {
        SkillInstallRecipe {
            id: "test".to_string(),
            name: "Test Recipe".to_string(),
            description: None,
            source: SkillInstallRecipeSource {
                source_type: SkillInstallRecipeSourceType::GitHub,
                repo: repo.to_string(),
                git_ref: "main".to_string(),
            },
            dirs: vec![SkillInstallRecipeDir { from: from.to_string(), to: to.to_string() }],
        }
    }

    #[test]
    fn test_parse_skill_install_recipe_json() {
        let recipe = parse_skill_install_recipe(
            r#"{
                "id": "vercel-react-best-practices",
                "name": "React Best Practices",
                "source": {
                    "type": "github",
                    "repo": "vercel-labs/agent-skills",
                    "ref": "main"
                },
                "dirs": [
                    {
                        "from": "skills/react-best-practices",
                        "to": "react-best-practices"
                    }
                ]
            }"#,
        )
        .unwrap();

        assert_eq!(recipe.id, "vercel-react-best-practices");
        assert_eq!(recipe.source.repo, "vercel-labs/agent-skills");
        assert_eq!(recipe.dirs[0].from, "skills/react-best-practices");
    }

    #[test]
    fn test_recipe_validation_rejects_nested_target_dir() {
        let recipe = create_recipe(
            "vercel-labs/agent-skills",
            "skills/react-best-practices",
            "nested/react",
        );
        let error = recipe.validate().unwrap_err();
        assert!(error.contains("single directory name"));
    }

    #[test]
    fn test_build_install_plan_for_nested_skill_directory() {
        let repo_root = TempDir::new().unwrap();
        let skills_root = TempDir::new().unwrap();
        let skill_dir = repo_root.path().join("plugins/expo-app-design/skills/building-native-ui");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), "# Skill").unwrap();

        let recipe = create_recipe(
            "expo/skills",
            "plugins/expo-app-design/skills/building-native-ui",
            "building-native-ui",
        );

        let plan = build_install_plan(
            &recipe,
            repo_root.path(),
            skills_root.path(),
            "https://codeload.github.com/expo/skills/zip/main",
        )
        .unwrap();

        assert_eq!(plan.skills.len(), 1);
        assert_eq!(plan.skills[0].from, "plugins/expo-app-design/skills/building-native-ui");
        assert_eq!(plan.skills[0].to, "building-native-ui");
        assert_eq!(plan.skills[0].detected_entry_file, "SKILL.md");
    }

    #[test]
    fn test_install_recipe_normalizes_uppercase_skill_file_name() {
        let repo_root = TempDir::new().unwrap();
        let skills_root = TempDir::new().unwrap();
        let skill_dir = repo_root.path().join("security");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.MD"), "# Security skill").unwrap();

        let recipe = create_recipe("better-auth/skills", "security", "security");
        let result = install_recipe_from_repo_root(
            &recipe,
            repo_root.path(),
            skills_root.path(),
            "https://codeload.github.com/better-auth/skills/zip/main",
        )
        .unwrap();

        let installed_dir = skills_root.path().join("security");
        assert!(installed_dir.join("SKILL.md").exists());
        assert!(contains_exact_file_name(&installed_dir, "SKILL.md").unwrap());
        assert!(!contains_exact_file_name(&installed_dir, "SKILL.MD").unwrap());
        assert_eq!(
            find_skill_entry_file_for_current_scanner(&installed_dir)
                .unwrap()
                .unwrap()
                .file_name()
                .and_then(|name| name.to_str()),
            Some("SKILL.md")
        );
        assert_eq!(result.installed_skills[0].normalized_entry_file, "SKILL.md");
    }

    #[test]
    fn test_install_recipe_replaces_existing_target_directory() {
        let repo_root = TempDir::new().unwrap();
        let skills_root = TempDir::new().unwrap();
        let skill_dir = repo_root.path().join("skills/react-best-practices");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), "# New skill").unwrap();

        let existing_dir = skills_root.path().join("react-best-practices");
        fs::create_dir_all(&existing_dir).unwrap();
        fs::write(existing_dir.join("obsolete.txt"), "old").unwrap();

        let recipe = create_recipe(
            "vercel-labs/agent-skills",
            "skills/react-best-practices",
            "react-best-practices",
        );
        install_recipe_from_repo_root(
            &recipe,
            repo_root.path(),
            skills_root.path(),
            "https://codeload.github.com/vercel-labs/agent-skills/zip/main",
        )
        .unwrap();

        assert!(existing_dir.join("SKILL.md").exists());
        assert!(!existing_dir.join("obsolete.txt").exists());
    }

    #[test]
    fn test_load_skill_install_recipe_from_file() {
        let dir = TempDir::new().unwrap();
        let recipe_path = dir.path().join("recipe.json");
        let mut file = fs::File::create(&recipe_path).unwrap();
        writeln!(
            file,
            r#"{{
                "id": "supabase-postgres-best-practices",
                "name": "Supabase Postgres Best Practices",
                "source": {{
                    "type": "github",
                    "repo": "supabase/agent-skills",
                    "ref": "main"
                }},
                "dirs": [
                    {{
                        "from": "skills/supabase-postgres-best-practices",
                        "to": "supabase-postgres-best-practices"
                    }}
                ]
            }}"#
        )
        .unwrap();

        let recipe = load_skill_install_recipe_from_file(&recipe_path).unwrap();
        assert_eq!(recipe.dirs[0].to, "supabase-postgres-best-practices");
    }
}
