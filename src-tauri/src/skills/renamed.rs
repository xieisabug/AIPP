//! Official AIPP skills renamed in the AIPP-skills repository (2.0.0).
//!
//! A skill identifier is `agents:<folder>`, so a folder rename strands the old copy and every
//! assistant binding to it. Once the new folder is installed, the old folder is removed only when
//! its SKILL.md is byte-identical to a published official version (never a local edit), and
//! bindings move to the new identifier when the old folder is gone.

use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

pub(crate) struct RenamedSkill {
    pub old: &'static str,
    pub new: &'static str,
    /// SHA-256 of every SKILL.md the old folder was ever published with.
    official_digests: &'static [&'static str],
}

pub(crate) const RENAMED_OFFICIAL_SKILLS: &[RenamedSkill] = &[
    RenamedSkill {
        old: "aipp-artifact",
        new: "building-aipp-artifacts",
        official_digests: &[
            "b8b2cd138b4adcecdd79310160c73c7ec1892f5026c23aaa163b3240bdce9fb9",
            "476b78b19aec65020a3c3df07ad789093a0218984d69bb09a9724999db6ee1bd",
        ],
    },
    RenamedSkill {
        old: "aipp-database",
        new: "managing-aipp-data",
        official_digests: &[
            "f2f42ca16d31a9d582abafff9b17984a6519060a01df6c56dc97e344ca57a5f6",
            "cf16d3fb5689052257507839eb726c087d1da7cba0e5d642f2685f5fee3f7afe",
        ],
    },
    RenamedSkill {
        old: "aipp-plugin-dev",
        new: "developing-aipp-plugins",
        official_digests: &["4223780d1c2ef55bf2ca7750bfcc14dc3566315ba1aaeae2fe2561ae91ae9226"],
    },
];

fn installed(skills_dir: &Path, folder: &str) -> bool {
    skills_dir.join(folder).join("SKILL.md").is_file()
}

fn is_unmodified_official(skills_dir: &Path, skill: &RenamedSkill) -> bool {
    let folder = skills_dir.join(skill.old);
    // A symlink points at something the user manages elsewhere; never follow it to delete.
    let Ok(meta) = fs::symlink_metadata(&folder) else { return false };
    if !meta.is_dir() {
        return false;
    }
    let Ok(bytes) = fs::read(folder.join("SKILL.md")) else { return false };
    let digest = hex::encode(Sha256::digest(bytes));
    skill.official_digests.contains(&digest.as_str())
}

/// Remove old official folders that a newer official folder replaces. Returns removed folder names.
pub(crate) fn retire_replaced_folders(skills_dir: &Path) -> Vec<&'static str> {
    retire_in(skills_dir, RENAMED_OFFICIAL_SKILLS)
}

fn retire_in(skills_dir: &Path, renamed: &'static [RenamedSkill]) -> Vec<&'static str> {
    let mut removed = Vec::new();
    for skill in renamed {
        if !installed(skills_dir, skill.new) || !is_unmodified_official(skills_dir, skill) {
            continue;
        }
        match fs::remove_dir_all(skills_dir.join(skill.old)) {
            Ok(()) => removed.push(skill.old),
            Err(error) => tracing::warn!(old = skill.old, %error, "Failed to remove renamed official skill folder"),
        }
    }
    removed
}

/// Move `agents:<old>` bindings to `agents:<new>` when only the new folder exists.
/// An assistant already bound to both keeps the new row; returns the number of rows changed.
pub(crate) fn rebind_assistant_skills(conn: &Connection, skills_dir: &Path) -> rusqlite::Result<usize> {
    rebind_in(conn, skills_dir, RENAMED_OFFICIAL_SKILLS)
}

fn rebind_in(conn: &Connection, skills_dir: &Path, renamed: &[RenamedSkill]) -> rusqlite::Result<usize> {
    let mut changed = 0;
    for skill in renamed {
        if !installed(skills_dir, skill.new) || skills_dir.join(skill.old).exists() {
            continue;
        }
        let old = format!("agents:{}", skill.old);
        let new = format!("agents:{}", skill.new);
        let tx = conn.unchecked_transaction()?;
        changed += tx.execute(
            "DELETE FROM assistant_skill_config WHERE skill_identifier = ?1 AND assistant_id IN
               (SELECT assistant_id FROM assistant_skill_config WHERE skill_identifier = ?2)",
            params![old, new],
        )?;
        changed += tx.execute(
            "UPDATE assistant_skill_config SET skill_identifier = ?2 WHERE skill_identifier = ?1",
            params![old, new],
        )?;
        tx.commit()?;
    }
    Ok(changed)
}

#[cfg(test)]
#[path = "renamed_tests.rs"]
mod tests;
