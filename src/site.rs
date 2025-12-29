use crate::render::{first_heading_title, render_markdown_file};
use crate::template::Template;
use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub struct RenderOptions<'a> {
    pub live_reload: bool,
    pub template: &'a Template,
}

#[derive(Clone)]
struct PageEntry {
    rel_path: PathBuf,
    output_rel: PathBuf,
    title: String,
    is_index: bool,
    is_readme: bool,
}

struct SiteMap {
    pages_by_dir: HashMap<PathBuf, Vec<PageEntry>>,
    pages_by_path: HashMap<PathBuf, PageEntry>,
    index_dirs: HashSet<PathBuf>,
    landing_dirs: HashSet<PathBuf>,
}

pub fn build_site(input: &Path, output: &Path, options: &RenderOptions<'_>) -> Result<()> {
    std::fs::create_dir_all(output)
        .with_context(|| format!("Failed to create output directory {}", output.display()))?;

    let site_map = build_site_map(input);

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
            let rendered = render_markdown_file(path, input, &site_map.index_dirs)?;
            let rel_path = rel_path.to_path_buf();
            let page_entry = match site_map.pages_by_path.get(&rel_path) {
                Some(entry) => entry,
                None => continue,
            };
            let extra_body = if options.live_reload {
                Some(live_reload_script())
            } else {
                None
            };
            let nav_html = build_nav_html(page_entry, &site_map);
            let breadcrumbs_html = build_breadcrumbs_html(page_entry, &site_map);
            let full_html = options
                .template
                .render(
                    &page_entry.title,
                    &rendered.html,
                    &nav_html,
                    &breadcrumbs_html,
                    None,
                    extra_body,
                );
            let out_path = output.join(&page_entry.output_rel);
            write_html(&out_path, &full_html)?;
            if page_entry.is_readme {
                if should_write_index(path, input, &site_map.index_dirs) {
                    let index_path = output
                        .join(rel_path.parent().unwrap_or(Path::new("")))
                        .join("index.html");
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

pub fn check_site(input: &Path) -> Result<usize> {
    let site_map = build_site_map(input);
    let mut warnings = 0usize;

    for entry in WalkDir::new(input).into_iter().filter_map(Result::ok) {
        let path = entry.path();
        if path == input {
            continue;
        }

        if is_markdown(path) {
            let rendered = render_markdown_file(path, input, &site_map.index_dirs)?;
            for warning in rendered.warnings {
                eprintln!("Warning: {warning}");
                warnings += 1;
            }
        }
    }

    Ok(warnings)
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

pub fn output_rel_path(
    path: &Path,
    input_root: &Path,
    index_dirs: &HashSet<PathBuf>,
) -> Option<PathBuf> {
    if !is_markdown(path) {
        return None;
    }
    let rel = path.strip_prefix(input_root).ok()?;
    if is_readme(path) && should_write_index(path, input_root, index_dirs) {
        Some(rel.parent().unwrap_or(Path::new("")).join("index.html"))
    } else {
        Some(rel.with_extension("html"))
    }
}

fn write_html(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create output directory {}", parent.display()))?;
    }
    std::fs::write(path, content)
        .with_context(|| format!("Failed to write output file {}", path.display()))
}

fn build_site_map(input: &Path) -> SiteMap {
    let mut pages_by_dir: HashMap<PathBuf, Vec<PageEntry>> = HashMap::new();
    let mut pages_by_path: HashMap<PathBuf, PageEntry> = HashMap::new();
    let mut index_dirs = HashSet::new();
    let mut landing_dirs = HashSet::new();

    for entry in WalkDir::new(input).into_iter().filter_map(Result::ok) {
        if entry.file_type().is_file() && is_markdown(entry.path()) {
            let path = entry.path();
            let rel_path = match path.strip_prefix(input) {
                Ok(rel) => rel.to_path_buf(),
                Err(_) => continue,
            };
            let rel_dir = rel_path.parent().unwrap_or(Path::new("")).to_path_buf();
            let is_index = is_index(path);
            let is_readme = is_readme(path);
            let title = title_from_markdown(path);
            let output_rel = rel_path.with_extension("html");
            let page = PageEntry {
                rel_path: rel_path.clone(),
                output_rel,
                title,
                is_index,
                is_readme,
            };
            pages_by_dir
                .entry(rel_dir.clone())
                .or_default()
                .push(page.clone());
            pages_by_path.insert(rel_path, page);
            if is_index {
                index_dirs.insert(rel_dir.clone());
            }
            if is_index || is_readme {
                landing_dirs.insert(rel_dir);
            }
        }
    }

    for pages in pages_by_dir.values_mut() {
        pages.sort_by(|a, b| a.title.cmp(&b.title));
    }

    SiteMap {
        pages_by_dir,
        pages_by_path,
        index_dirs,
        landing_dirs,
    }
}

pub fn collect_index_dirs(input: &Path) -> HashSet<PathBuf> {
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

fn build_nav_html(current: &PageEntry, site_map: &SiteMap) -> String {
    let current_dir = current.rel_path.parent().unwrap_or(Path::new(""));
    let from_dir = current.output_rel.parent().unwrap_or(Path::new(""));
    let mut nav = String::new();

    let pages = site_map.pages_by_dir.get(current_dir);
    let mut page_items = Vec::new();
    if let Some(pages) = pages {
        for page in pages {
            if page.rel_path == current.rel_path {
                continue;
            }
            let href = relative_link(from_dir, &page.output_rel);
            let label = html_escape(&page.title);
            page_items.push(format!(r#"<li><a href="{}">{}</a></li>"#, href, label));
        }
    }

    let mut folder_items = Vec::new();
    for dir in &site_map.landing_dirs {
        if dir == current_dir {
            continue;
        }
        if dir.parent().unwrap_or(Path::new("")) == current_dir {
            let folder_label = landing_title(dir, site_map).unwrap_or_else(|| display_dir_name(dir));
            let target = dir.join("index.html");
            let href = relative_link(from_dir, &target);
            folder_items.push(format!(
                r#"<li><a href="{}">{}</a></li>"#,
                href,
                html_escape(&folder_label)
            ));
        }
    }
    folder_items.sort();

    if !page_items.is_empty() {
        nav.push_str(r#"<div class="nav-section">"#);
        nav.push_str(r#"<div class="nav-title">Pages</div>"#);
        nav.push_str(r#"<ul class="nav-list">"#);
        for item in page_items {
            nav.push_str(&item);
        }
        nav.push_str("</ul></div>");
    }

    if !folder_items.is_empty() {
        nav.push_str(r#"<div class="nav-section">"#);
        nav.push_str(r#"<div class="nav-title">Folders</div>"#);
        nav.push_str(r#"<ul class="nav-list">"#);
        for item in folder_items {
            nav.push_str(&item);
        }
        nav.push_str("</ul></div>");
    }

    nav
}

fn build_breadcrumbs_html(current: &PageEntry, site_map: &SiteMap) -> String {
    let mut crumbs = Vec::new();
    let current_dir = current.rel_path.parent().unwrap_or(Path::new(""));
    let from_dir = current.output_rel.parent().unwrap_or(Path::new(""));
    let is_landing = current.is_index || current.is_readme;

    let ancestors = ancestor_dirs(current_dir);
    for dir in ancestors {
        if dir == current_dir && is_landing {
            continue;
        }
        if site_map.landing_dirs.contains(&dir) {
            let label = if dir.as_os_str().is_empty() {
                "Home".to_string()
            } else {
                landing_title(&dir, site_map).unwrap_or_else(|| display_dir_name(&dir))
            };
            let target = dir.join("index.html");
            let href = relative_link(from_dir, &target);
            crumbs.push(format!(
                r#"<a href="{}">{}</a>"#,
                href,
                html_escape(&label)
            ));
        }
    }

    crumbs.push(format!(r#"<span>{}</span>"#, html_escape(&current.title)));

    let mut html = String::new();
    for (idx, crumb) in crumbs.iter().enumerate() {
        if idx > 0 {
            html.push_str(r#"<span class="sep">/</span>"#);
        }
        html.push_str(crumb);
    }
    html
}

fn ancestor_dirs(dir: &Path) -> Vec<PathBuf> {
    let mut ancestors = Vec::new();
    let mut current = PathBuf::new();
    ancestors.push(PathBuf::new());
    for component in dir.components() {
        current.push(component.as_os_str());
        ancestors.push(current.clone());
    }
    ancestors
}

fn relative_link(from_dir: &Path, target: &Path) -> String {
    let from_parts = path_parts(from_dir);
    let to_parts = path_parts(target);
    let mut common = 0usize;
    while common < from_parts.len()
        && common < to_parts.len()
        && from_parts[common] == to_parts[common]
    {
        common += 1;
    }

    let mut parts = Vec::new();
    for _ in common..from_parts.len() {
        parts.push("..".to_string());
    }
    for part in &to_parts[common..] {
        parts.push(part.clone());
    }

    if parts.is_empty() {
        ".".to_string()
    } else {
        parts.join("/")
    }
}

fn path_parts(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|component| match component {
            std::path::Component::Normal(os) => Some(os.to_string_lossy().to_string()),
            std::path::Component::ParentDir => Some("..".to_string()),
            std::path::Component::CurDir => None,
            _ => None,
        })
        .collect()
}

fn title_from_markdown(path: &Path) -> String {
    if let Ok(contents) = std::fs::read_to_string(path) {
        if let Some(title) = first_heading_title(&contents) {
            return title;
        }
    }
    display_title(path)
}

fn display_title(path: &Path) -> String {
    let stem = path
        .file_stem()
        .and_then(OsStr::to_str)
        .unwrap_or("Document");
    stem.replace(['-', '_'], " ")
}

fn display_dir_name(path: &Path) -> String {
    path.file_name()
        .and_then(OsStr::to_str)
        .unwrap_or("Folder")
        .replace(['-', '_'], " ")
}

fn landing_title(dir: &Path, site_map: &SiteMap) -> Option<String> {
    let pages = site_map.pages_by_dir.get(dir)?;
    if let Some(index) = pages.iter().find(|page| page.is_index) {
        return Some(index.title.clone());
    }
    if let Some(readme) = pages.iter().find(|page| page.is_readme) {
        return Some(readme.title.clone());
    }
    None
}

fn html_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
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
        assert!(html.contains("class=\"sidebar\""));
        assert!(html.contains("class=\"breadcrumbs\""));

        let asset_path = output_dir.path().join("assets/logo.txt");
        let asset = std::fs::read_to_string(asset_path).expect("read asset");
        assert_eq!(asset, "logo");
    }

    #[test]
    fn builds_nav_and_breadcrumbs() {
        let input_dir = tempdir().expect("input tempdir");

        std::fs::write(input_dir.path().join("README.md"), "# Root").expect("root readme");
        let docs_dir = input_dir.path().join("docs");
        std::fs::create_dir_all(&docs_dir).expect("docs dir");
        std::fs::write(docs_dir.join("README.md"), "# Docs").expect("docs readme");
        std::fs::write(docs_dir.join("intro.md"), "# Intro").expect("intro");

        let guide_dir = docs_dir.join("guide");
        std::fs::create_dir_all(&guide_dir).expect("guide dir");
        std::fs::write(guide_dir.join("index.md"), "# Guide").expect("guide index");
        std::fs::write(guide_dir.join("extra.md"), "# Extra").expect("extra page");

        let sub_dir = guide_dir.join("sub");
        std::fs::create_dir_all(&sub_dir).expect("sub dir");
        std::fs::write(sub_dir.join("README.md"), "# Subsection").expect("sub readme");

        let site_map = build_site_map(input_dir.path());
        let current = site_map
            .pages_by_path
            .get(&PathBuf::from("docs/guide/extra.md"))
            .expect("current page");

        let nav = build_nav_html(current, &site_map);
        assert!(nav.contains("Pages"));
        assert!(nav.contains("Guide"));
        assert!(!nav.contains("intro.html"));
        assert!(nav.contains("Subsection"));

        let breadcrumbs = build_breadcrumbs_html(current, &site_map);
        assert!(breadcrumbs.contains("Home"));
        assert!(breadcrumbs.contains("Docs"));
        assert!(breadcrumbs.contains("Guide"));
        assert!(breadcrumbs.contains("Extra"));
    }

    #[test]
    fn excludes_current_folder_from_nav_folders() {
        let input_dir = tempdir().expect("input tempdir");
        let docs_dir = input_dir.path().join("docs");
        std::fs::create_dir_all(&docs_dir).expect("docs dir");

        let guide_dir = docs_dir.join("guide");
        std::fs::create_dir_all(&guide_dir).expect("guide dir");
        std::fs::write(guide_dir.join("index.md"), "# Guide").expect("guide index");

        let current_path = guide_dir.join("page.md");
        std::fs::write(&current_path, "# Page").expect("page");

        let sub_dir = guide_dir.join("sub");
        std::fs::create_dir_all(&sub_dir).expect("sub dir");
        std::fs::write(sub_dir.join("README.md"), "# Subsection").expect("sub readme");
        let site_map = build_site_map(input_dir.path());
        let current = site_map
            .pages_by_path
            .get(&PathBuf::from("docs/guide/page.md"))
            .expect("current page");

        let nav = build_nav_html(current, &site_map);
        assert!(nav.contains("Subsection"));
        assert_eq!(nav.matches("Guide</a>").count(), 1);
    }

    #[test]
    fn sorts_pages_and_folders_by_title() {
        let input_dir = tempdir().expect("input tempdir");
        let docs_dir = input_dir.path().join("docs");
        std::fs::create_dir_all(&docs_dir).expect("docs dir");
        std::fs::write(docs_dir.join("README.md"), "# Docs").expect("docs readme");

        std::fs::write(docs_dir.join("zeta.md"), "# Zeta").expect("zeta");
        std::fs::write(docs_dir.join("alpha.md"), "# Alpha").expect("alpha");

        let alpha_dir = docs_dir.join("alpha-folder");
        std::fs::create_dir_all(&alpha_dir).expect("alpha dir");
        std::fs::write(alpha_dir.join("README.md"), "# Alpha Folder").expect("alpha readme");

        let zeta_dir = docs_dir.join("zeta-folder");
        std::fs::create_dir_all(&zeta_dir).expect("zeta dir");
        std::fs::write(zeta_dir.join("README.md"), "# Zeta Folder").expect("zeta readme");

        let site_map = build_site_map(input_dir.path());
        let current = site_map
            .pages_by_path
            .get(&PathBuf::from("docs/README.md"))
            .expect("current page");

        let nav = build_nav_html(current, &site_map);
        let alpha_index = nav.find("Alpha</a>").expect("alpha page");
        let zeta_index = nav.find("Zeta</a>").expect("zeta page");
        assert!(alpha_index < zeta_index);

        let alpha_folder = nav.find("Alpha Folder</a>").expect("alpha folder");
        let zeta_folder = nav.find("Zeta Folder</a>").expect("zeta folder");
        assert!(alpha_folder < zeta_folder);
    }
}
