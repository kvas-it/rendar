use crate::render::render_markdown_file;
use crate::template::Template;
use anyhow::{Context, Result};
use std::ffi::OsStr;
use std::path::Path;
use walkdir::WalkDir;

pub struct RenderOptions<'a> {
    pub live_reload: bool,
    pub template: &'a Template,
}

pub fn build_site(input: &Path, output: &Path, options: &RenderOptions<'_>) -> Result<()> {
    std::fs::create_dir_all(output)
        .with_context(|| format!("Failed to create output directory {}", output.display()))?;

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
            let body_html = render_markdown_file(path)?;
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
                .render(title, &body_html, None, extra_body);
            let out_path = output.join(rel_path).with_extension("html");
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent).with_context(|| {
                    format!(
                        "Failed to create output directory {}",
                        parent.display()
                    )
                })?;
            }
            std::fs::write(&out_path, full_html).with_context(|| {
                format!("Failed to write output file {}", out_path.display())
            })?;
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
