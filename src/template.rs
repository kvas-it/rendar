use anyhow::{Context, Result};
use std::path::Path;

pub struct Template {
    raw: String,
    style: String,
}

impl Template {
    pub fn built_in() -> Self {
        Self {
            raw: include_str!("../assets/theme/template.html").to_string(),
            style: include_str!("../assets/theme/style.css").to_string(),
        }
    }

    pub fn from_path(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read template {}", path.display()))?;
        Ok(Self {
            raw,
            style: String::new(),
        })
    }

    pub fn render(
        &self,
        title: &str,
        content: &str,
        nav: &str,
        breadcrumbs: &str,
        extra_head: Option<&str>,
        extra_body: Option<&str>,
    ) -> String {
        let mut html = self.raw.clone();
        html = html.replace("{{title}}", title);
        html = html.replace("{{content}}", content);
        html = html.replace("{{nav}}", nav);
        html = html.replace("{{breadcrumbs}}", breadcrumbs);
        html = html.replace("{{style}}", &self.style);
        html = html.replace("{{extra_head}}", extra_head.unwrap_or(""));
        html = html.replace("{{extra_body}}", extra_body.unwrap_or(""));
        html
    }
}
