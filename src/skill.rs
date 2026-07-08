//! The bundled agent skill.
//!
//! `mail-cli` ships its own usage skill compiled into the binary, so an agent
//! can install it with `mail-cli install-skill` without cloning the repo. The
//! asset bytes live in `skill/` and are embedded at build time; this module is
//! the only place that materializes them to disk. Like everything else here it
//! is stateless: it just writes bundled files to a directory you name.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

/// The skill's canonical name; also the default install-directory leaf.
pub const NAME: &str = "mail-cli";

/// Every bundled asset as `(path relative to the skill root, contents)`.
///
/// Paths use forward slashes; they are re-joined per-component so the layout is
/// reproduced correctly on any platform.
const ASSETS: &[(&str, &str)] = &[
    ("SKILL.md", include_str!("../skill/SKILL.md")),
    (
        "references/reference.md",
        include_str!("../skill/references/reference.md"),
    ),
    (
        "evals/evals.json",
        include_str!("../skill/evals/evals.json"),
    ),
];

/// The default install root: `~/.agents/skills/mail-cli`.
///
/// Resolves the home directory from `HOME` (Unix) or `USERPROFILE` (Windows).
pub fn default_dir() -> Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .context("cannot locate home directory: neither HOME nor USERPROFILE is set")?;
    Ok(Path::new(&home).join(".agents").join("skills").join(NAME))
}

/// Write the bundled skill into `dir`, creating parent directories as needed.
///
/// Refuses to clobber an existing install unless `force` is set (fails loud,
/// naming the file). Returns the paths written, in bundle order.
pub fn install(dir: &Path, force: bool) -> Result<Vec<PathBuf>> {
    let mut written = Vec::with_capacity(ASSETS.len());
    for (rel, contents) in ASSETS {
        // Rejoin per component so embedded '/' separators are portable.
        let mut dest = dir.to_path_buf();
        for part in rel.split('/') {
            dest.push(part);
        }
        if dest.exists() && !force {
            bail!(
                "{} already exists; pass --force to overwrite",
                dest.display()
            );
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("cannot create directory {}", parent.display()))?;
        }
        std::fs::write(&dest, contents)
            .with_context(|| format!("cannot write {}", dest.display()))?;
        written.push(dest);
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assets_are_embedded_and_nonempty() {
        assert_eq!(ASSETS.len(), 3);
        for (rel, contents) in ASSETS {
            assert!(!contents.is_empty(), "{rel} should be embedded non-empty");
        }
    }

    #[test]
    fn skill_md_has_frontmatter_name() {
        let (_, skill_md) = ASSETS[0];
        assert!(skill_md.contains("name: mail-cli"));
    }

    #[test]
    fn install_writes_every_asset() {
        let tmp = std::env::temp_dir().join(format!("mailcli-skilltest-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);

        let written = install(&tmp, false).unwrap();
        assert_eq!(written.len(), ASSETS.len());
        assert!(tmp.join("SKILL.md").is_file());
        assert!(tmp.join("references/reference.md").is_file());
        assert!(tmp.join("evals/evals.json").is_file());

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn install_refuses_overwrite_without_force() {
        let tmp = std::env::temp_dir().join(format!("mailcli-skillforce-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);

        install(&tmp, false).unwrap();
        // Second install without force must fail loud.
        assert!(install(&tmp, false).is_err());
        // With force it succeeds.
        assert!(install(&tmp, true).is_ok());

        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
