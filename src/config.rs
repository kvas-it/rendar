use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Deserialize)]
pub struct Config {
    pub input: Option<PathBuf>,
    pub template: Option<PathBuf>,
    pub exclude: Option<Vec<String>>,
    pub preview: Option<PreviewConfig>,
}

#[derive(Debug, Default, Deserialize)]
pub struct PreviewConfig {
    pub port: Option<u16>,
    pub open: Option<bool>,
}

impl Config {
    fn resolve_paths(&mut self, base: &Path) {
        if let Some(path) = self.input.as_mut() {
            *path = resolve_path(base, path);
        }
        if let Some(path) = self.template.as_mut() {
            *path = resolve_path(base, path);
        }
    }
}

pub fn load_config(path: Option<&Path>) -> Result<Option<Config>> {
    let config_path = match path {
        Some(path) => Some(path.to_path_buf()),
        None => {
            let candidate = PathBuf::from("rendar.toml");
            if candidate.exists() {
                Some(candidate)
            } else {
                None
            }
        }
    };

    let Some(config_path) = config_path else {
        return Ok(None);
    };

    let raw = std::fs::read_to_string(&config_path)
        .with_context(|| format!("Failed to read config {}", config_path.display()))?;
    let mut config: Config =
        toml::from_str(&raw).context("Failed to parse rendar.toml")?;
    let base_dir = config_path.parent().unwrap_or(Path::new("."));
    config.resolve_paths(base_dir);
    Ok(Some(config))
}

fn resolve_path(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn loads_and_resolves_paths() {
        let dir = tempdir().expect("tempdir");
        let config_path = dir.path().join("rendar.toml");
        let content = r#"
input = "docs"
template = "theme.html"
exclude = ["AGENTS.md", "CLAUDE.md"]

[preview]
port = 4040
open = true
"#;
        std::fs::write(&config_path, content).expect("write config");
        let config = load_config(Some(&config_path)).expect("load config");
        let config = config.expect("config should exist");
        assert_eq!(config.input.unwrap(), dir.path().join("docs"));
        assert_eq!(config.template.unwrap(), dir.path().join("theme.html"));
        assert_eq!(
            config.exclude.unwrap(),
            vec!["AGENTS.md".to_string(), "CLAUDE.md".to_string()]
        );
        let preview = config.preview.expect("preview config");
        assert_eq!(preview.port, Some(4040));
        assert_eq!(preview.open, Some(true));
    }
}
