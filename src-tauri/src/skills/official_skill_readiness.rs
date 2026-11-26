use super::*;
use sha2::{Digest, Sha256};

/// Remote identity only. The installed entry and marker are read afresh on every check.
pub(crate) struct OfficialSkillEntry {
    target: String,
    pub(super) source_id: String,
    entry: String,
    digest: [u8; 32],
}

impl OfficialSkillEntry {
    pub(super) fn from_plan(source: &SkillInstallRecipeSource, skill: &SkillInstallPlanSkill, repo_root: &Path) -> Result<Self, String> {
        let from = resolve_recipe_dir(repo_root, &skill.from)?.join(&skill.detected_entry_file);
        let bytes = fs::read(&from).map_err(|e| format!("读取官方 Skills 入口 {} 失败：{e}", from.display()))?;
        Ok(Self {
            target: skill.to.clone(), source_id: format!("{}#{}", source.source_label(), skill.from),
            entry: skill.normalized_entry_file.clone(), digest: Sha256::digest(bytes).into(),
        })
    }

    pub(crate) fn installed_at(&self, skills_dir: &Path) -> Result<bool, String> {
        let target = skills_dir.join(&self.target);
        match fs::symlink_metadata(&target) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(format!("读取 Skill {} 失败：{e}", target.display())),
            Ok(_) => {}
        }
        let entry = target.join(&self.entry);
        let same = match fs::read_to_string(target.join(".aipp-install-source")) {
            Ok(marker) => marker == self.source_id && entry.is_file(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => fs::read(entry)
                .is_ok_and(|bytes| <[u8; 32]>::from(Sha256::digest(bytes)) == self.digest),
            Err(e) => return Err(format!("读取 Skill {} 来源记录失败：{e}", self.target)),
        };
        if same { Ok(true) } else { Err(format!("Skill {} 已有其他来源、缺少入口或存在本地修改，保留现有目录", self.target)) }
    }
}

fn archive_manifest(source: &SkillInstallRecipeSource, dirs: Option<&[SkillInstallRecipeDir]>, root: &Path, target: &Path) -> Result<Vec<OfficialSkillEntry>, String> {
    let skills = build_archive_plan_skills(source, dirs, root, target)?;
    if skills.is_empty() { return Err(format!("{} 中未找到 SKILL.md", source.source_label())); }
    skills.iter().map(|skill| OfficialSkillEntry::from_plan(source, skill, root)).collect()
}

pub(crate) fn official_skills_ready(entries: &[OfficialSkillEntry], skills_dir: &Path) -> Result<bool, String> {
    let mut ready = !entries.is_empty();
    for entry in entries {
        if !entry.installed_at(skills_dir)? { ready = false; }
    }
    Ok(ready)
}

pub(crate) async fn read_official_archive_manifest(source: &SkillInstallRecipeSource, dirs: &[SkillInstallRecipeDir], skills_dir: &Path, proxy: Option<&str>) -> Result<Vec<OfficialSkillEntry>, String> {
    let scratch = tempfile::tempdir().map_err(|e| format!("创建 Skills 检测临时目录失败：{e}"))?;
    let archive = download_and_extract_source(source, proxy, scratch.path()).await?;
    archive_manifest(source, Some(dirs), &archive.repo_root, skills_dir)
}

#[cfg(test)]
#[path = "official_skill_readiness_tests.rs"]
mod tests;
