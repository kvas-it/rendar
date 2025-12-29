use crate::render::render_markdown_file;
use crate::template::Template;
use anyhow::{Context, Result};
use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub struct RenderOptions<'a> {
    pub live_reload: bool,
    pub template: &'a Template,
}

pub fn build_site(input: &Path, output: &Path, options: &RenderOptions<'_>) -> Result<()> {
    std::fs::create_dir_all(output)
        .with_context(|| format!("Failed to create output directory {}", output.display()))?;

    let index_dirs = collect_index_dirs(input);

    for entry in WalkDir::new(input).into_iter().filter_map(Result::ok) {
        let path = entry.path();
        if path == input {
            continue;
        }

        if is_within(path, output) {
            continue;
        }

        let rel_path = path.strip_prefix(input).with_context(|| {
            format!(
                "Failed to compute relative path for {}",
                path.display()
            )
        })?;

        if entry.file_type().is_dir() {
            let out_dir = output.join(rel_path);
            std::fs::create_dir_all(&out_dir).with_context(|| {
                format!(
                    "Failed to create output directory {}",
                    out_dir.display()
                )
            })?;
            continue;
        }

        if is_markdown(path) {
            let rendered = render_markdown_file(path, input, &index_dirs)?;
            let title = path
                .file_stem()
                .and_then(OsStr::to_str)
                .unwrap_or("Document");
            let extra_body = if options.live_reload {
                Some(live_reload_script())
            } else {
                None
            };
            let full_html = options
                .template
                .render(title, &rendered.html, None, extra_body);
            let out_path = output.join(rel_path).with_extension("html");
            write_html(&out_path, &full_html)?;
            if is_readme(path) {
                if should_write_index(path, input, &index_dirs) {
                    let index_path = output.join(rel_path.parent().unwrap_or(Path::new(""))).join("index.html");
                    write_html(&index_path, &full_html)?;
                }
            }
            for warning in rendered.warnings {
                eprintln!("Warning: {warning}");
            }
        } else {
            let out_path = output.join(rel_path);
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent).with_context(|| {
                    format!(
                        "Failed to create output directory {}",
                        parent.display()
                    )
                })?;
            }
            std::fs::copy(path, &out_path).with_context(|| {
                format!(
                    "Failed to copy asset from {} to {}",
                    path.display(),
                    out_path.display()
                )
            })?;
        }
    }

    Ok(())
}

fn is_markdown(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(OsStr::to_str)
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref(),
        Some("md") | Some("markdown")
    )
}

fn is_readme(path: &Path) -> bool {
    path.file_stem()
        .and_then(OsStr::to_str)
        .map(|stem| stem.eq_ignore_ascii_case("readme"))
        .unwrap_or(false)
        && is_markdown(path)
}

fn is_index(path: &Path) -> bool {
    path.file_stem()
        .and_then(OsStr::to_str)
        .map(|stem| stem.eq_ignore_ascii_case("index"))
        .unwrap_or(false)
        && is_markdown(path)
}

fn write_html(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create output directory {}", parent.display()))?;
    }
    std::fs::write(path, content)
        .with_context(|| format!("Failed to write output file {}", path.display()))
}

fn collect_index_dirs(input: &Path) -> HashSet<PathBuf> {
    let mut dirs = HashSet::new();
    for entry in WalkDir::new(input).into_iter().filter_map(Result::ok) {
        if entry.file_type().is_file() && is_index(entry.path()) {
            if let Ok(rel) = entry.path().parent().unwrap_or(input).strip_prefix(input) {
                dirs.insert(rel.to_path_buf());
            } else {
                dirs.insert(PathBuf::new());
            }
        }
    }
    dirs
}

fn should_write_index(path: &Path, input: &Path, index_dirs: &HashSet<PathBuf>) -> bool {
    let parent = path.parent().unwrap_or(input);
    let rel = parent.strip_prefix(input).unwrap_or(parent);
    !index_dirs.contains(rel)
}

fn is_within(path: &Path, root: &Path) -> bool {
    let path = match path.canonicalize() {
        Ok(p) => p,
        Err(_) => return false,
    };
    let root = match root.canonicalize() {
        Ok(p) => p,
        Err(_) => return false,
    };
    path.starts_with(root)
}

fn live_reload_script() -> &'static str {
    r#"<script>
(function () {
  const endpoint = "/__rendar_version";
  let last = null;
  async function poll() {
    try {
      const res = await fetch(endpoint, { cache: "no-store" });
      const text = await res.text();
      if (last === null) {
        last = text;
      } else if (last !== text) {
        location.reload();
        return;
      }
    } catch (_) {}
    setTimeout(poll, 1000);
  }
  poll();
})();
</script>
"#
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn builds_html_and_copies_assets() {
        let input_dir = tempdir().expect("input tempdir");
        let output_dir = tempdir().expect("output tempdir");

        let docs_dir = input_dir.path().join("docs");
        std::fs::create_dir_all(&docs_dir).expect("create docs dir");
        std::fs::write(docs_dir.join("index.md"), "# Hello").expect("write markdown");

        let assets_dir = input_dir.path().join("assets");
        std::fs::create_dir_all(&assets_dir).expect("create assets dir");
        std::fs::write(assets_dir.join("logo.txt"), "logo").expect("write asset");

        let template = Template::built_in();
        build_site(
            input_dir.path(),
            output_dir.path(),
            &RenderOptions {
                live_reload: false,
                template: &template,
            },
        )
        .expect("build site");

        let html_path = output_dir.path().join("docs/index.html");
        let html = std::fs::read_to_string(html_path).expect("read html");
        assert!(html.contains("<!doctype html>"));
        assert!(html.contains("Hello"));

        let asset_path = output_dir.path().join("assets/logo.txt");
        let asset = std::fs::read_to_string(asset_path).expect("read asset");
        assert_eq!(asset, "logo");
    }
}
