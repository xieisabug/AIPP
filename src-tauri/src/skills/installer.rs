use crate::skills::parser::SkillParser;
use crate::skills::types::SkillMetadata;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
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
    #[serde(rename = "type", alias = "source_type")]
    pub source_type: SkillInstallRecipeSourceType,
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(rename = "ref", alias = "git_ref", default = "default_git_ref")]
    pub git_ref: String,
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SkillInstallRecipeSourceType {
    #[serde(rename = "github", alias = "git_hub")]
    GitHub,
    #[serde(rename = "zip")]
    Zip,
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
    pub display_name: String,
    pub detected_entry_file: String,
    pub normalized_entry_file: String,
    pub will_replace: bool,
    pub metadata: SkillMetadata,
    pub preview: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillInstallPlan {
    pub recipe_id: String,
    pub recipe_name: String,
    pub source: SkillInstallRecipeSource,
    pub source_label: String,
    pub download_url: String,
    pub target_directory: String,
    pub skills: Vec<SkillInstallPlanSkill>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillInstallResult {
    pub recipe_id: String,
    pub recipe_name: String,
    pub source: SkillInstallRecipeSource,
    pub source_label: String,
    pub download_url: String,
    pub target_directory: String,
    pub installed_skills: Vec<SkillInstallPlanSkill>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillArchiveInspection {
    pub source: SkillInstallRecipeSource,
    pub source_label: String,
    pub download_url: String,
    pub target_directory: String,
    pub skills: Vec<SkillInstallPlanSkill>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillArchiveInstallResult {
    pub source: SkillInstallRecipeSource,
    pub source_label: String,
    pub download_url: String,
    pub target_directory: String,
    pub installed_skills: Vec<SkillInstallPlanSkill>,
}

#[derive(Debug, Clone)]
struct DiscoveredSkillCandidate {
    from: String,
    source_dir: PathBuf,
    entry_file: PathBuf,
}

const ARCHIVE_DOWNLOAD_TIMEOUT_SECS: u64 = 30;
const MAX_DISCOVERY_DEPTH: usize = 6;
const IGNORED_DISCOVERY_SEGMENTS: &[&str] = &[
    ".git",
    ".github",
    ".claude-plugin",
    ".codex",
    ".cursor",
    ".vscode",
    "assets",
    "coverage",
    "dist",
    "doc",
    "docs",
    "example",
    "examples",
    "images",
    "img",
    "node_modules",
    "reference",
    "references",
    "script",
    "scripts",
    "target",
    "template",
    "templates",
    "test",
    "tests",
    "vendor",
    "__pycache__",
];

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
        validate_skill_install_dirs(&self.dirs, true)?;
        Ok(())
    }
}

pub fn validate_skill_install_dirs(
    dirs: &[SkillInstallRecipeDir],
    require_non_empty: bool,
) -> Result<(), String> {
    if require_non_empty && dirs.is_empty() {
        return Err("Recipe must contain at least one directory mapping".to_string());
    }

    let mut seen_targets = HashSet::new();
    for dir in dirs {
        validate_relative_source_path(&dir.from)?;
        validate_target_dir_name(&dir.to)?;

        if !seen_targets.insert(dir.to.clone()) {
            return Err(format!("Duplicate target directory in recipe: {}", dir.to));
        }
    }

    Ok(())
}

impl SkillInstallRecipeSource {
    pub fn validate(&self) -> Result<(), String> {
        match self.source_type {
            SkillInstallRecipeSourceType::GitHub => {
                let repo = self
                    .repo
                    .as_deref()
                    .ok_or_else(|| "GitHub source requires repo".to_string())?;
                validate_github_repo(repo)?;

                if self.git_ref.trim().is_empty() {
                    return Err("Git ref cannot be empty".to_string());
                }
            }
            SkillInstallRecipeSourceType::Zip => {
                let url =
                    self.url.as_deref().ok_or_else(|| "Zip source requires url".to_string())?;
                validate_zip_url(url)?;
            }
        }

        Ok(())
    }

    fn archive_url(&self) -> Result<String, String> {
        self.validate()?;

        match self.source_type {
            SkillInstallRecipeSourceType::GitHub => Ok(format!(
                "https://codeload.github.com/{}/zip/{}",
                self.repo.as_deref().unwrap_or_default(),
                self.git_ref
            )),
            SkillInstallRecipeSourceType::Zip => Ok(self.url.clone().unwrap_or_default()),
        }
    }

    pub fn source_label(&self) -> String {
        match self.source_type {
            SkillInstallRecipeSourceType::GitHub => {
                format!("{}#{}", self.repo.as_deref().unwrap_or_default(), self.git_ref)
            }
            SkillInstallRecipeSourceType::Zip => self.url.clone().unwrap_or_default(),
        }
    }

    fn target_dir_fallback_name(&self, repo_root: Option<&Path>) -> String {
        match self.source_type {
            SkillInstallRecipeSourceType::GitHub => self
                .repo
                .as_deref()
                .and_then(|repo| repo.split('/').next_back())
                .map(strip_archive_suffix)
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "skill".to_string()),
            SkillInstallRecipeSourceType::Zip => {
                if let Some(repo_root) = repo_root {
                    if let Some(file_name) = repo_root.file_name().and_then(|name| name.to_str()) {
                        let normalized = strip_archive_suffix(file_name);
                        if !normalized.is_empty() {
                            return normalized;
                        }
                    }
                }

                self.url
                    .as_deref()
                    .and_then(|url| reqwest::Url::parse(url).ok())
                    .and_then(|parsed| {
                        parsed
                            .path_segments()
                            .and_then(|mut segments| {
                                segments.by_ref().filter(|segment| !segment.is_empty()).next_back()
                            })
                            .map(strip_archive_suffix)
                    })
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| "skill".to_string())
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
    proxy_url: Option<&str>,
) -> Result<SkillInstallPlan, String> {
    let archive = download_and_extract_source(&recipe.source, proxy_url).await?;
    build_install_plan(recipe, &archive.repo_root, skills_dir, &archive.download_url)
}

pub async fn install_skill_install_recipe(
    recipe: &SkillInstallRecipe,
    skills_dir: &Path,
    proxy_url: Option<&str>,
) -> Result<SkillInstallResult, String> {
    let archive = download_and_extract_source(&recipe.source, proxy_url).await?;
    install_recipe_from_repo_root(recipe, &archive.repo_root, skills_dir, &archive.download_url)
}

pub async fn inspect_skill_archive(
    source: &SkillInstallRecipeSource,
    configured_dirs: Option<&[SkillInstallRecipeDir]>,
    skills_dir: &Path,
    proxy_url: Option<&str>,
) -> Result<SkillArchiveInspection, String> {
    let archive = download_and_extract_source(source, proxy_url).await?;
    let skills =
        build_archive_plan_skills(source, configured_dirs, &archive.repo_root, skills_dir)?;

    Ok(SkillArchiveInspection {
        source: source.clone(),
        source_label: source.source_label(),
        download_url: archive.download_url,
        target_directory: skills_dir.to_string_lossy().to_string(),
        skills,
    })
}

pub async fn install_skill_archive(
    source: &SkillInstallRecipeSource,
    selections: &[SkillInstallRecipeDir],
    skills_dir: &Path,
    proxy_url: Option<&str>,
) -> Result<SkillArchiveInstallResult, String> {
    if selections.is_empty() {
        return Err("At least one skill must be selected for installation".to_string());
    }

    let recipe = SkillInstallRecipe {
        id: build_archive_recipe_id(source),
        name: source.source_label(),
        description: None,
        source: source.clone(),
        dirs: selections.to_vec(),
    };
    recipe.validate()?;

    let archive = download_and_extract_source(source, proxy_url).await?;
    let result = install_recipe_from_repo_root(
        &recipe,
        &archive.repo_root,
        skills_dir,
        &archive.download_url,
    )?;

    Ok(SkillArchiveInstallResult {
        source: result.source,
        source_label: result.source_label,
        download_url: result.download_url,
        target_directory: result.target_directory,
        installed_skills: result.installed_skills,
    })
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

struct DownloadedSkillArchive {
    _temp_dir: TempExtractDir,
    repo_root: PathBuf,
    download_url: String,
}

async fn download_and_extract_source(
    source: &SkillInstallRecipeSource,
    proxy_url: Option<&str>,
) -> Result<DownloadedSkillArchive, String> {
    source.validate()?;

    let download_url = source.archive_url()?;
    let client = build_archive_http_client(proxy_url)?;

    let response = client
        .get(&download_url)
        .timeout(std::time::Duration::from_secs(ARCHIVE_DOWNLOAD_TIMEOUT_SECS))
        .send()
        .await
        .map_err(|e| format_archive_download_error(source, &download_url, &e))?;

    if !response.status().is_success() {
        return Err(format!(
            "下载技能压缩包失败：{} 返回 HTTP {}。可以尝试检查链接或使用代理后重试",
            download_url,
            response.status()
        ));
    }

    let archive_bytes = response
        .bytes()
        .await
        .map_err(|e| format!("读取下载的技能压缩包失败：{}（{}）", download_url, e))?;

    let temp_dir = TempExtractDir::new("skill_recipe_extract")?;
    extract_zip_bytes_to_dir(archive_bytes.as_ref(), &temp_dir.path)?;
    let repo_root = resolve_archive_root(&temp_dir.path)?;

    Ok(DownloadedSkillArchive { _temp_dir: temp_dir, repo_root, download_url })
}

fn build_archive_http_client(proxy_url: Option<&str>) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder().user_agent("AIPP skill installer");

    if let Some(proxy_url) = proxy_url {
        let proxy = reqwest::Proxy::all(proxy_url).map_err(|e| format!("代理配置失败: {}", e))?;
        builder = builder.proxy(proxy);
    }

    builder.build().map_err(|e| format!("Failed to build HTTP client: {}", e))
}

fn format_archive_download_error(
    source: &SkillInstallRecipeSource,
    download_url: &str,
    error: &reqwest::Error,
) -> String {
    let source_label = source.source_label();

    if error.is_timeout() {
        format!(
            "下载技能压缩包超时（{} 秒）：{}（{}）。可以尝试使用代理后重试",
            ARCHIVE_DOWNLOAD_TIMEOUT_SECS, source_label, download_url
        )
    } else if error.is_connect() {
        format!(
            "连接技能压缩包地址失败：{}（{}）。请检查网络或尝试使用代理后重试",
            source_label, download_url
        )
    } else {
        format!("下载技能压缩包失败：{}（{}）：{}", source_label, download_url, error)
    }
}

fn build_install_plan(
    recipe: &SkillInstallRecipe,
    repo_root: &Path,
    skills_dir: &Path,
    download_url: &str,
) -> Result<SkillInstallPlan, String> {
    recipe.validate()?;
    let skills = build_plan_skills_from_dir_mappings(&recipe.dirs, repo_root, skills_dir)?;

    let source_label = recipe.source.source_label();

    Ok(SkillInstallPlan {
        recipe_id: recipe.id.clone(),
        recipe_name: recipe.name.clone(),
        source: recipe.source.clone(),
        source_label,
        download_url: download_url.to_string(),
        target_directory: skills_dir.to_string_lossy().to_string(),
        skills,
    })
}

fn build_archive_plan_skills(
    source: &SkillInstallRecipeSource,
    configured_dirs: Option<&[SkillInstallRecipeDir]>,
    repo_root: &Path,
    skills_dir: &Path,
) -> Result<Vec<SkillInstallPlanSkill>, String> {
    if let Some(dirs) = configured_dirs.filter(|dirs| !dirs.is_empty()) {
        return build_plan_skills_from_dir_mappings(dirs, repo_root, skills_dir);
    }

    let candidates = discover_archive_skill_candidates(repo_root)?;
    if candidates.is_empty() {
        return Err(
            "未在仓库或 ZIP 包中识别到可安装的 Skill 目录。当前仅识别包含 SKILL.md 的 Skill 目录。"
                .to_string(),
        );
    }

    let target_names = assign_discovered_target_names(&candidates, source);
    let mut skills = Vec::with_capacity(candidates.len());

    for candidate in candidates {
        let to = target_names
            .get(&candidate.from)
            .cloned()
            .ok_or_else(|| format!("Missing target directory name for {}", candidate.from))?;
        skills.push(build_plan_skill(
            &candidate.from,
            &to,
            &candidate.source_dir,
            &candidate.entry_file,
            skills_dir,
        )?);
    }

    skills.sort_by(|left, right| left.from.cmp(&right.from));
    Ok(skills)
}

fn build_plan_skills_from_dir_mappings(
    dirs: &[SkillInstallRecipeDir],
    repo_root: &Path,
    skills_dir: &Path,
) -> Result<Vec<SkillInstallPlanSkill>, String> {
    validate_skill_install_dirs(dirs, true)?;

    let mut skills = Vec::with_capacity(dirs.len());
    for dir in dirs {
        let source_dir = resolve_recipe_dir(repo_root, &dir.from)?;
        let entry_file = find_skill_entry_file_case_insensitive(&source_dir)?
            .ok_or_else(|| format!("Directory {} does not contain SKILL.md", dir.from))?;

        skills.push(build_plan_skill(&dir.from, &dir.to, &source_dir, &entry_file, skills_dir)?);
    }

    Ok(skills)
}

fn build_plan_skill(
    from: &str,
    to: &str,
    _source_dir: &Path,
    entry_file: &Path,
    skills_dir: &Path,
) -> Result<SkillInstallPlanSkill, String> {
    let detected_entry_file = entry_file
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("Invalid UTF-8 file name in {}", entry_file.display()))?
        .to_string();
    let normalized_entry_file = normalized_entry_file_name(entry_file)?;
    let metadata = SkillParser::parse_metadata(entry_file).map_err(|e| {
        format!("Failed to parse skill metadata from {}: {}", entry_file.display(), e)
    })?;
    let fallback_name = fallback_skill_name(from, to);
    let display_name =
        resolve_skill_display_name(metadata.name.clone(), entry_file, &fallback_name);
    let preview = build_skill_preview(entry_file, metadata.description.clone())?;

    Ok(SkillInstallPlanSkill {
        from: from.to_string(),
        to: to.to_string(),
        display_name,
        detected_entry_file,
        normalized_entry_file,
        will_replace: skills_dir.join(to).exists(),
        metadata,
        preview,
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
                "Installed skill {} but AIPP scanner still cannot detect SKILL.md",
                skill.to
            ));
        }
    }

    Ok(SkillInstallResult {
        recipe_id: plan.recipe_id,
        recipe_name: plan.recipe_name,
        source: plan.source,
        source_label: plan.source_label,
        download_url: plan.download_url,
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

fn validate_zip_url(url: &str) -> Result<(), String> {
    let parsed =
        reqwest::Url::parse(url.trim()).map_err(|e| format!("Zip source url is invalid: {}", e))?;

    match parsed.scheme() {
        "http" | "https" => Ok(()),
        scheme => Err(format!("Zip source url must use http or https, got scheme: {}", scheme)),
    }
}

fn validate_relative_source_path(path: &str) -> Result<(), String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("Recipe source directory cannot be empty".to_string());
    }

    if trimmed == "." {
        return Ok(());
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

    let resolved = if from.trim() == "." { repo_root.to_path_buf() } else { repo_root.join(from) };
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

fn discover_archive_skill_candidates(
    repo_root: &Path,
) -> Result<Vec<DiscoveredSkillCandidate>, String> {
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    discover_archive_skill_candidates_recursive(
        repo_root,
        repo_root,
        0,
        &mut candidates,
        &mut seen,
    )?;
    candidates.sort_by(|left, right| left.from.cmp(&right.from));
    Ok(candidates)
}

fn discover_archive_skill_candidates_recursive(
    repo_root: &Path,
    current_dir: &Path,
    depth: usize,
    candidates: &mut Vec<DiscoveredSkillCandidate>,
    seen: &mut HashSet<String>,
) -> Result<(), String> {
    let is_root = current_dir == repo_root;
    if let Some(candidate) = build_discovered_skill_candidate(repo_root, current_dir, is_root)? {
        if seen.insert(candidate.from.clone()) {
            candidates.push(candidate);
        }
        if !is_root {
            return Ok(());
        }
    }

    if depth >= MAX_DISCOVERY_DEPTH {
        return Ok(());
    }

    for entry in collect_directory_entries(current_dir)? {
        if !entry.is_dir() || should_skip_discovery_dir(&entry) {
            continue;
        }

        discover_archive_skill_candidates_recursive(
            repo_root,
            &entry,
            depth + 1,
            candidates,
            seen,
        )?;
    }

    Ok(())
}

fn build_discovered_skill_candidate(
    repo_root: &Path,
    dir: &Path,
    allow_root: bool,
) -> Result<Option<DiscoveredSkillCandidate>, String> {
    let entry_file = match find_skill_entry_file_case_insensitive(dir)? {
        Some(entry_file) => entry_file,
        None => return Ok(None),
    };

    let relative_from = relative_archive_path(repo_root, dir)?;
    if !should_include_discovered_skill_dir(&relative_from, &entry_file, allow_root) {
        return Ok(None);
    }

    Ok(Some(DiscoveredSkillCandidate {
        from: relative_from,
        source_dir: dir.to_path_buf(),
        entry_file,
    }))
}

fn should_skip_discovery_dir(dir: &Path) -> bool {
    let Some(name) = dir.file_name().and_then(|value| value.to_str()) else {
        return true;
    };

    let lower = name.to_ascii_lowercase();
    name.starts_with('.') || IGNORED_DISCOVERY_SEGMENTS.contains(&lower.as_str())
}

fn should_include_discovered_skill_dir(
    relative_from: &str,
    entry_file: &Path,
    allow_root: bool,
) -> bool {
    if relative_from == "." {
        return allow_root && file_name_eq(entry_file, "SKILL.md");
    }

    let segments = split_relative_path(relative_from);
    if segments
        .iter()
        .map(|segment| segment.to_ascii_lowercase())
        .any(|segment| IGNORED_DISCOVERY_SEGMENTS.contains(&segment.as_str()))
    {
        return false;
    }

    file_name_eq(entry_file, "SKILL.md")
}

fn relative_archive_path(repo_root: &Path, dir: &Path) -> Result<String, String> {
    let relative = dir.strip_prefix(repo_root).map_err(|e| {
        format!("Failed to determine archive-relative path for {}: {}", dir.display(), e)
    })?;

    let mut components = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(value) => components.push(value.to_string_lossy().to_string()),
            Component::CurDir => {}
            _ => {
                return Err(format!(
                    "Archive path {} contains unsupported path segments",
                    dir.display()
                ));
            }
        }
    }

    if components.is_empty() {
        Ok(".".to_string())
    } else {
        Ok(components.join("/"))
    }
}

fn assign_discovered_target_names(
    candidates: &[DiscoveredSkillCandidate],
    source: &SkillInstallRecipeSource,
) -> HashMap<String, String> {
    let initial_names: HashMap<String, String> = candidates
        .iter()
        .map(|candidate| {
            (candidate.from.clone(), default_discovered_target_name(candidate, source))
        })
        .collect();

    let mut counts = HashMap::new();
    for name in initial_names.values() {
        *counts.entry(name.clone()).or_insert(0usize) += 1;
    }

    let mut used = HashSet::new();
    let mut assigned = HashMap::new();

    for candidate in candidates {
        let base_name =
            initial_names.get(&candidate.from).cloned().unwrap_or_else(|| "skill".to_string());
        let mut proposed = if counts.get(&base_name).copied().unwrap_or_default() > 1 {
            disambiguated_discovered_target_name(candidate, source)
        } else {
            base_name.clone()
        };

        if proposed.trim().is_empty() {
            proposed = "skill".to_string();
        }

        let unique_base = proposed.clone();
        let mut suffix = 2usize;
        while !used.insert(proposed.clone()) {
            proposed = format!("{}-{}", unique_base, suffix);
            suffix += 1;
        }

        assigned.insert(candidate.from.clone(), proposed);
    }

    assigned
}

fn default_discovered_target_name(
    candidate: &DiscoveredSkillCandidate,
    source: &SkillInstallRecipeSource,
) -> String {
    if candidate.from == "." {
        return source.target_dir_fallback_name(Some(&candidate.source_dir));
    }

    Path::new(&candidate.from)
        .file_name()
        .and_then(|value| value.to_str())
        .map(strip_archive_suffix)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "skill".to_string())
}

fn disambiguated_discovered_target_name(
    candidate: &DiscoveredSkillCandidate,
    source: &SkillInstallRecipeSource,
) -> String {
    if candidate.from == "." {
        return source.target_dir_fallback_name(Some(&candidate.source_dir));
    }

    let joined = split_relative_path(&candidate.from)
        .into_iter()
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let normalized = strip_archive_suffix(&joined);
    if normalized.is_empty() {
        "skill".to_string()
    } else {
        normalized
    }
}

fn find_skill_entry_file_case_insensitive(skill_dir: &Path) -> Result<Option<PathBuf>, String> {
    let entries = collect_directory_entries(skill_dir)?;
    let files: Vec<PathBuf> = entries.into_iter().filter(|path| path.is_file()).collect();

    Ok(files.iter().find(|path| file_name_eq(path, "SKILL.md")).cloned())
}

fn find_skill_entry_file_for_current_scanner(skill_dir: &Path) -> Result<Option<PathBuf>, String> {
    let entries = collect_directory_entries(skill_dir)?;
    let files: Vec<PathBuf> = entries.into_iter().filter(|path| path.is_file()).collect();

    Ok(files
        .iter()
        .find(|path| path.file_name().and_then(|name| name.to_str()) == Some("SKILL.md"))
        .cloned())
}

fn normalized_entry_file_name(entry_path: &Path) -> Result<String, String> {
    let file_name = entry_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("Invalid UTF-8 file name in {}", entry_path.display()))?;

    if file_name.eq_ignore_ascii_case("SKILL.md") {
        return Ok("SKILL.md".to_string());
    }

    Err(format!("Unsupported skill entry file {}; only SKILL.md is allowed", entry_path.display()))
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

fn fallback_skill_name(from: &str, to: &str) -> String {
    if from == "." {
        return to.to_string();
    }

    Path::new(from)
        .file_name()
        .and_then(|value| value.to_str())
        .map(|value| value.to_string())
        .unwrap_or_else(|| to.to_string())
}

fn resolve_skill_display_name(
    metadata_name: Option<String>,
    skill_file: &Path,
    fallback_name: &str,
) -> String {
    match metadata_name {
        Some(name) => {
            let inferred_from_filename = skill_file
                .file_stem()
                .and_then(|stem| stem.to_str())
                .map(|stem| stem.eq_ignore_ascii_case(name.as_str()))
                .unwrap_or(false);

            if inferred_from_filename {
                fallback_name.to_string()
            } else {
                name
            }
        }
        None => fallback_name.to_string(),
    }
}

fn build_skill_preview(
    entry_file: &Path,
    metadata_description: Option<String>,
) -> Result<Option<String>, String> {
    if let Some(description) = metadata_description {
        let trimmed = description.trim();
        if !trimmed.is_empty() {
            return Ok(Some(trimmed.to_string()));
        }
    }

    let content = fs::read_to_string(entry_file).map_err(|e| {
        format!("Failed to read skill entry file {} for preview: {}", entry_file.display(), e)
    })?;

    Ok(extract_markdown_preview(&content))
}

fn extract_markdown_preview(content: &str) -> Option<String> {
    let body = strip_frontmatter(content);
    let mut parts = Vec::new();

    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !parts.is_empty() {
                break;
            }
            continue;
        }
        if trimmed == "---" {
            continue;
        }
        if parts.is_empty() && trimmed.starts_with('#') {
            continue;
        }

        let cleaned = trimmed.trim_start_matches(['-', '*', '>', ' ']).trim();
        if cleaned.is_empty() {
            continue;
        }

        parts.push(cleaned.to_string());
        if parts.join(" ").chars().count() >= 220 {
            break;
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(truncate_preview(&parts.join(" "), 240))
    }
}

fn strip_frontmatter(content: &str) -> &str {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return content;
    }

    let after_first = &trimmed[3..];
    if let Some(end_pos) = after_first.find("\n---") {
        let rest = &after_first[end_pos + 4..];
        rest.trim_start_matches('\n')
    } else {
        content
    }
}

fn truncate_preview(content: &str, max_chars: usize) -> String {
    let mut result = String::new();
    for (index, ch) in content.chars().enumerate() {
        if index >= max_chars {
            result.push('…');
            return result;
        }
        result.push(ch);
    }
    result
}

fn split_relative_path(path: &str) -> Vec<String> {
    path.split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .map(|segment| segment.to_string())
        .collect()
}

fn strip_archive_suffix(name: &str) -> String {
    let mut value = name.trim().trim_end_matches('/').to_string();
    if let Some(stripped) = value.strip_suffix(".zip") {
        value = stripped.to_string();
    }

    for suffix in ["-main", "-master", "-head"] {
        if let Some(stripped) = value.strip_suffix(suffix) {
            if !stripped.trim().is_empty() {
                value = stripped.to_string();
                break;
            }
        }
    }

    value
}

fn build_archive_recipe_id(source: &SkillInstallRecipeSource) -> String {
    let fallback = source.target_dir_fallback_name(None);
    let sanitized = fallback
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' { ch } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string();

    if sanitized.is_empty() {
        "archive-skill".to_string()
    } else {
        format!("archive-{}", sanitized)
    }
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
                repo: Some(repo.to_string()),
                git_ref: "main".to_string(),
                url: None,
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
        assert_eq!(recipe.source.repo.as_deref(), Some("vercel-labs/agent-skills"));
        assert_eq!(recipe.dirs[0].from, "skills/react-best-practices");
    }

    #[test]
    fn test_parse_zip_skill_install_recipe_json() {
        let recipe = parse_skill_install_recipe(
            r#"{
                "id": "shared-skill-zip",
                "name": "Shared Skill Zip",
                "source": {
                    "type": "zip",
                    "url": "https://example.com/shared-skills.zip"
                },
                "dirs": [
                    {
                        "from": "skills/shared-skill",
                        "to": "shared-skill"
                    }
                ]
            }"#,
        )
        .unwrap();

        assert_eq!(recipe.source.source_type, SkillInstallRecipeSourceType::Zip);
        assert_eq!(recipe.source.url.as_deref(), Some("https://example.com/shared-skills.zip"));
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
    fn test_recipe_validation_rejects_zip_without_url() {
        let recipe = SkillInstallRecipe {
            id: "zip-test".to_string(),
            name: "Zip Test".to_string(),
            description: None,
            source: SkillInstallRecipeSource {
                source_type: SkillInstallRecipeSourceType::Zip,
                repo: None,
                git_ref: "main".to_string(),
                url: None,
            },
            dirs: vec![SkillInstallRecipeDir {
                from: "skills/example".to_string(),
                to: "example".to_string(),
            }],
        };

        let error = recipe.validate().unwrap_err();
        assert!(error.contains("Zip source requires url"));
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
    fn test_build_archive_plan_skills_discovers_only_skill_md_layouts() {
        let repo_root = TempDir::new().unwrap();
        let skills_root = TempDir::new().unwrap();

        let skill_a = repo_root.path().join("skills/react-best-practices");
        fs::create_dir_all(&skill_a).unwrap();
        fs::write(skill_a.join("SKILL.md"), "# React\n\nReact best practices").unwrap();

        let skill_b = repo_root.path().join("plugins/expo-app-design/skills/building-native-ui");
        fs::create_dir_all(&skill_b).unwrap();
        fs::write(skill_b.join("SKILL.md"), "# Expo UI\n\nBuild native interfaces").unwrap();

        let skill_c = repo_root.path().join("engineering/skill-tester");
        fs::create_dir_all(&skill_c).unwrap();
        fs::write(skill_c.join("SKILL.md"), "# Skill Tester\n\nTest a skill quickly").unwrap();

        let readme_only_dir = repo_root.path().join("security/readme-only");
        fs::create_dir_all(&readme_only_dir).unwrap();
        fs::write(readme_only_dir.join("README.md"), "# Readme Only\n\nShould be ignored").unwrap();

        let docs_dir = repo_root.path().join("docs/getting-started");
        fs::create_dir_all(&docs_dir).unwrap();
        fs::write(docs_dir.join("README.md"), "# Docs\n\nNot a skill").unwrap();

        let source = SkillInstallRecipeSource {
            source_type: SkillInstallRecipeSourceType::GitHub,
            repo: Some("aipp-org/example-skills".to_string()),
            git_ref: "main".to_string(),
            url: None,
        };

        let skills =
            build_archive_plan_skills(&source, None, repo_root.path(), skills_root.path()).unwrap();
        let discovered_paths = skills.iter().map(|skill| skill.from.clone()).collect::<Vec<_>>();

        assert_eq!(
            discovered_paths,
            vec![
                "engineering/skill-tester".to_string(),
                "plugins/expo-app-design/skills/building-native-ui".to_string(),
                "skills/react-best-practices".to_string(),
            ]
        );
    }

    #[test]
    fn test_build_archive_plan_skills_uses_configured_dirs_when_present() {
        let repo_root = TempDir::new().unwrap();
        let skills_root = TempDir::new().unwrap();

        let configured_skill = repo_root.path().join("bundles/security-review");
        fs::create_dir_all(&configured_skill).unwrap();
        fs::write(configured_skill.join("SKILL.md"), "# Security Review").unwrap();

        let auto_skill = repo_root.path().join("skills/auto-discovered");
        fs::create_dir_all(&auto_skill).unwrap();
        fs::write(auto_skill.join("SKILL.md"), "# Auto Discovered").unwrap();

        let source = SkillInstallRecipeSource {
            source_type: SkillInstallRecipeSourceType::GitHub,
            repo: Some("aipp-org/configured-bundle".to_string()),
            git_ref: "main".to_string(),
            url: None,
        };
        let configured_dirs = vec![SkillInstallRecipeDir {
            from: "bundles/security-review".to_string(),
            to: "security-review".to_string(),
        }];

        let skills = build_archive_plan_skills(
            &source,
            Some(&configured_dirs),
            repo_root.path(),
            skills_root.path(),
        )
        .unwrap();

        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].from, "bundles/security-review");
        assert_eq!(skills[0].to, "security-review");
    }

    #[test]
    fn test_build_archive_plan_skills_disambiguates_duplicate_target_names() {
        let repo_root = TempDir::new().unwrap();
        let skills_root = TempDir::new().unwrap();

        let foo_security = repo_root.path().join("foo/security");
        fs::create_dir_all(&foo_security).unwrap();
        fs::write(foo_security.join("SKILL.md"), "# Foo Security").unwrap();

        let bar_security = repo_root.path().join("bar/security");
        fs::create_dir_all(&bar_security).unwrap();
        fs::write(bar_security.join("SKILL.md"), "# Bar Security").unwrap();

        let source = SkillInstallRecipeSource {
            source_type: SkillInstallRecipeSourceType::GitHub,
            repo: Some("aipp-org/security-bundle".to_string()),
            git_ref: "main".to_string(),
            url: None,
        };

        let skills =
            build_archive_plan_skills(&source, None, repo_root.path(), skills_root.path()).unwrap();
        let target_dirs = skills.into_iter().map(|skill| skill.to).collect::<Vec<_>>();

        assert_eq!(target_dirs, vec!["bar-security".to_string(), "foo-security".to_string()]);
    }

    #[test]
    fn test_build_archive_plan_skills_supports_root_skill_directory() {
        let repo_root = TempDir::new().unwrap();
        let skills_root = TempDir::new().unwrap();
        fs::write(repo_root.path().join("SKILL.md"), "# Root Skill\n\nOne root skill").unwrap();

        let source = SkillInstallRecipeSource {
            source_type: SkillInstallRecipeSourceType::GitHub,
            repo: Some("aipp-org/root-skill".to_string()),
            git_ref: "main".to_string(),
            url: None,
        };

        let skills =
            build_archive_plan_skills(&source, None, repo_root.path(), skills_root.path()).unwrap();

        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].from, ".");
        assert_eq!(skills[0].to, "root-skill");
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
