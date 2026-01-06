use anyhow::{Context, Result};
use axum::extract::State;
use clap::{Parser, Subcommand};
use globset::{Glob, GlobSet, GlobSetBuilder};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

mod config;
mod render;
mod site;
mod template;

#[derive(Parser)]
#[command(name = "rendar", version, about = "Render a Markdown tree into a static HTML site")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Render Markdown files into a static HTML output directory.
    Build {
        /// Output directory for generated HTML.
        #[arg(short, long)]
        out: PathBuf,
        /// Input directory to render (defaults to current directory).
        #[arg(short, long)]
        input: Option<PathBuf>,
        /// Optional config file path (e.g., rendar.toml).
        #[arg(short, long)]
        config: Option<PathBuf>,
        /// Optional template file path.
        #[arg(long)]
        template: Option<PathBuf>,
        /// Glob patterns to exclude from rendering (relative to input).
        #[arg(long, value_name = "PATTERN", action = clap::ArgAction::Append)]
        exclude: Vec<String>,
    },
    /// Check for broken links and other warnings without writing output.
    Check {
        /// Input directory to scan (defaults to current directory).
        #[arg(short, long)]
        input: Option<PathBuf>,
        /// Optional config file path (e.g., rendar.toml).
        #[arg(short, long)]
        config: Option<PathBuf>,
        /// Glob patterns to exclude from rendering (relative to input).
        #[arg(long, value_name = "PATTERN", action = clap::ArgAction::Append)]
        exclude: Vec<String>,
    },
    /// Start a local preview server with live reload.
    Preview {
        /// Input directory to render (defaults to current directory).
        #[arg(short, long)]
        input: Option<PathBuf>,
        /// Optional config file path (e.g., rendar.toml).
        #[arg(short, long)]
        config: Option<PathBuf>,
        /// Optional template file path.
        #[arg(long)]
        template: Option<PathBuf>,
        /// Start on a specific page or directory.
        #[arg(long)]
        start_on: Option<PathBuf>,
        /// Open the browser after starting the server.
        #[arg(long)]
        open: bool,
        /// Port for the preview server.
        #[arg(long)]
        port: Option<u16>,
        /// Glob patterns to exclude from rendering (relative to input).
        #[arg(long, value_name = "PATTERN", action = clap::ArgAction::Append)]
        exclude: Vec<String>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Build {
            out,
            input,
            config,
            template,
            exclude,
        } => run_build(out, input, config, template, exclude),
        Command::Check {
            input,
            config,
            exclude,
        } => run_check(input, config, exclude),
        Command::Preview {
            input,
            config,
            template,
            start_on,
            open,
            port,
            exclude,
        } => run_preview(input, config, template, start_on, open, port, exclude),
    }
}

fn run_build(
    out: PathBuf,
    input: Option<PathBuf>,
    config: Option<PathBuf>,
    template: Option<PathBuf>,
    exclude: Vec<String>,
) -> Result<()> {
    let config = config::load_config(config.as_deref())?;
    let input = resolve_input(input, config.as_ref());
    let template = resolve_template(template, config.as_ref());
    let template = load_template(template)?;
    let excludes = resolve_excludes(exclude, config.as_ref())?;
    site::build_site(
        &input,
        &out,
        &site::RenderOptions {
            live_reload: false,
            template: &template,
            exclude: excludes.as_ref(),
        },
    )?;
    println!("Rendered site to {}", out.display());
    Ok(())
}

fn run_check(input: Option<PathBuf>, config: Option<PathBuf>, exclude: Vec<String>) -> Result<()> {
    let config = config::load_config(config.as_deref())?;
    let input = resolve_input(input, config.as_ref());
    let excludes = resolve_excludes(exclude, config.as_ref())?;
    let warnings = site::check_site(&input, excludes.as_ref())?;
    if warnings > 0 {
        std::process::exit(1);
    }
    Ok(())
}

fn run_preview(
    input: Option<PathBuf>,
    config: Option<PathBuf>,
    template: Option<PathBuf>,
    start_on: Option<PathBuf>,
    open: bool,
    port: Option<u16>,
    exclude: Vec<String>,
) -> Result<()> {
    let config = config::load_config(config.as_deref())?;
    let input_override = input.or_else(|| config.as_ref().and_then(|cfg| cfg.input.clone()));
    let preview_paths = resolve_preview_paths(input_override, start_on)?;
    let input = preview_paths.input_root;
    let start_page = preview_paths.start_page;
    let template = resolve_template(template, config.as_ref());
    let template = load_template(template)?;
    let excludes = resolve_excludes(exclude, config.as_ref())?;
    if let Some(start_page) = start_page.as_ref() {
        if site::is_excluded_path(start_page, &input, excludes.as_ref()) {
            return Err(anyhow::anyhow!(
                "Start page {} is excluded by pattern",
                start_page.display()
            ));
        }
    }
    let temp_dir = tempfile::tempdir().context("Failed to create preview directory")?;
    let output = temp_dir.path().to_path_buf();
    site::build_site(
        &input,
        &output,
        &site::RenderOptions {
            live_reload: true,
            template: &template,
            exclude: excludes.as_ref(),
        },
    )?;

    let version = Arc::new(AtomicU64::new(1));
    let watcher_version = Arc::clone(&version);
    let input_clone = input.clone();
    let output_clone = output.clone();
    let watcher_excludes = excludes.clone();

    std::thread::spawn(move || {
        if let Err(err) = watch_and_rebuild(
            &input_clone,
            &output_clone,
            watcher_version,
            template,
            watcher_excludes,
        ) {
            eprintln!("Preview watcher error: {err}");
        }
    });

    let preferred_port = resolve_preview_port(port, config.as_ref());
    let (listener, port) = bind_preview_listener(preferred_port)?;
    let address = format!("127.0.0.1:{}", port);
    let start_url = if let Some(start_page) = start_page.as_ref() {
        let index_dirs = site::collect_index_dirs(&input, excludes.as_ref());
        match site::output_rel_path(start_page, &input, &index_dirs) {
            Some(rel) => format!("http://{address}/{}", path_to_url(&rel)),
            None => format!("http://{address}/"),
        }
    } else {
        format!("http://{address}/")
    };
    println!("Preview server running at {start_url}");
    let open = resolve_preview_open(open, config.as_ref());
    if open {
        open_browser(&start_url);
    }

    let rt = tokio::runtime::Runtime::new().context("Failed to start async runtime")?;
    rt.block_on(async move {
        let listener = tokio::net::TcpListener::from_std(listener)
            .context("Failed to use preview listener")?;
        serve_preview(output, version, listener).await
    })
}

fn watch_and_rebuild(
    input: &std::path::Path,
    output: &std::path::Path,
    version: Arc<AtomicU64>,
    template: template::Template,
    excludes: Option<GlobSet>,
) -> Result<()> {
    use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
    use std::sync::mpsc::channel;
    use std::time::Instant;

    let (tx, rx) = channel();
    let mut watcher = RecommendedWatcher::new(tx, Config::default())
        .context("Failed to initialize file watcher")?;
    watcher
        .watch(input, RecursiveMode::Recursive)
        .context("Failed to watch input directory")?;

    loop {
        let _ = rx.recv().context("File watcher channel closed")?;
        let start = Instant::now();
        while rx.recv_timeout(Duration::from_millis(200)).is_ok() {
            if start.elapsed() > Duration::from_secs(2) {
                break;
            }
        }
        if let Err(err) = site::build_site(
            input,
            output,
            &site::RenderOptions {
                live_reload: true,
                template: &template,
                exclude: excludes.as_ref(),
            },
        ) {
            eprintln!("Failed to rebuild preview: {err}");
        } else {
            version.fetch_add(1, Ordering::SeqCst);
        }
    }
}

fn load_template(path: Option<PathBuf>) -> Result<template::Template> {
    match path {
        Some(path) => template::Template::from_path(&path),
        None => Ok(template::Template::built_in()),
    }
}

fn resolve_input(input: Option<PathBuf>, config: Option<&config::Config>) -> PathBuf {
    input
        .or_else(|| config.and_then(|cfg| cfg.input.clone()))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn resolve_template(
    template: Option<PathBuf>,
    config: Option<&config::Config>,
) -> Option<PathBuf> {
    template.or_else(|| config.and_then(|cfg| cfg.template.clone()))
}

fn resolve_preview_port(port: Option<u16>, config: Option<&config::Config>) -> u16 {
    port.or_else(|| {
        config
            .and_then(|cfg| cfg.preview.as_ref())
            .and_then(|preview| preview.port)
    })
    .unwrap_or(3000)
}

fn bind_preview_listener(preferred_port: u16) -> Result<(std::net::TcpListener, u16)> {
    let address = ("127.0.0.1", preferred_port);
    let listener = match std::net::TcpListener::bind(address) {
        Ok(listener) => listener,
        Err(_err) if preferred_port == 3000 => {
            let fallback = std::net::TcpListener::bind(("127.0.0.1", 0)).with_context(|| {
                format!(
                    "Failed to bind preview server on {} and auto-select fallback port",
                    preferred_port
                )
            })?;
            eprintln!("Port {} is in use, picked a random available port.", preferred_port);
            fallback
        }
        Err(err) => {
            return Err(anyhow::anyhow!(
                "Failed to bind preview server on {}: {}",
                preferred_port,
                err
            ));
        }
    };

    listener
        .set_nonblocking(true)
        .context("Failed to set preview listener to non-blocking")?;
    let local_addr = listener
        .local_addr()
        .context("Failed to read preview listener address")?;
    let port = local_addr.port();
    Ok((listener, port))
}

fn resolve_preview_open(open: bool, config: Option<&config::Config>) -> bool {
    if open {
        true
    } else {
        config
            .and_then(|cfg| cfg.preview.as_ref())
            .and_then(|preview| preview.open)
            .unwrap_or(false)
    }
}

fn resolve_excludes(
    cli: Vec<String>,
    config: Option<&config::Config>,
) -> Result<Option<GlobSet>> {
    let patterns = if !cli.is_empty() {
        cli
    } else {
        config
            .and_then(|cfg| cfg.exclude.clone())
            .unwrap_or_default()
    };

    if patterns.is_empty() {
        return Ok(None);
    }

    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        let glob = Glob::new(&pattern)
            .with_context(|| format!("Invalid exclude pattern: {}", pattern))?;
        builder.add(glob);
    }
    Ok(Some(builder.build()?))
}

struct PreviewPaths {
    input_root: PathBuf,
    start_page: Option<PathBuf>,
}

fn resolve_preview_paths(
    input_override: Option<PathBuf>,
    start_on: Option<PathBuf>,
) -> Result<PreviewPaths> {
    let cwd = std::env::current_dir().context("Failed to read current directory")?;
    resolve_preview_paths_with_cwd(&cwd, input_override, start_on)
}

fn resolve_preview_paths_with_cwd(
    cwd: &Path,
    input_override: Option<PathBuf>,
    start_on: Option<PathBuf>,
) -> Result<PreviewPaths> {
    let start_on = start_on.map(|path| resolve_path_from_cwd(cwd, path));
    let start_page = match start_on {
        Some(path) => Some(resolve_start_page(&path)?),
        None => None,
    };

    let input_root = if let Some(input) = input_override {
        resolve_path_from_cwd(cwd, input)
    } else if let Some(start_page) = start_page.as_ref() {
        if is_within(start_page, cwd) {
            cwd.to_path_buf()
        } else {
            discover_root_for_start(start_page)
        }
    } else {
        cwd.to_path_buf()
    };

    if let Some(start_page) = start_page.as_ref() {
        if !is_within(start_page, &input_root) {
            return Err(anyhow::anyhow!(
                "Start page {} is not under input root {}",
                start_page.display(),
                input_root.display()
            ));
        }
    }

    Ok(PreviewPaths {
        input_root,
        start_page,
    })
}

fn resolve_start_page(start_on: &Path) -> Result<PathBuf> {
    if start_on.is_dir() {
        find_landing_page(start_on).ok_or_else(|| {
            anyhow::anyhow!(
                "No index.md or README.md found in directory {}",
                start_on.display()
            )
        })
    } else {
        if !start_on.exists() {
            return Err(anyhow::anyhow!(
                "Start page {} does not exist",
                start_on.display()
            ));
        }
        if !is_markdown_file(start_on) {
            return Err(anyhow::anyhow!(
                "Start page {} is not a Markdown file",
                start_on.display()
            ));
        }
        Ok(start_on.to_path_buf())
    }
}

fn find_landing_page(dir: &Path) -> Option<PathBuf> {
    let candidates = [
        "index.md",
        "index.markdown",
        "README.md",
        "README.markdown",
    ];
    for name in candidates {
        let candidate = dir.join(name);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

fn discover_root_for_start(start_page: &Path) -> PathBuf {
    let mut current = start_page.parent().unwrap_or(start_page).to_path_buf();
    let mut found_index = false;
    let mut last_with_index = None;
    loop {
        if has_landing_page(&current) {
            found_index = true;
            last_with_index = Some(current.clone());
        } else if found_index {
            break;
        }

        let parent = match current.parent() {
            Some(parent) => parent.to_path_buf(),
            None => break,
        };
        current = parent;
    }
    last_with_index.unwrap_or_else(|| start_page.parent().unwrap_or(start_page).to_path_buf())
}

fn has_landing_page(dir: &Path) -> bool {
    let index = dir.join("index.md");
    let index_alt = dir.join("index.markdown");
    let readme = dir.join("README.md");
    let readme_alt = dir.join("README.markdown");
    index.exists() || index_alt.exists() || readme.exists() || readme_alt.exists()
}

fn resolve_path_from_cwd(cwd: &Path, path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    }
}

fn is_markdown_file(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref(),
        Some("md") | Some("markdown")
    )
}

fn is_within(path: &Path, root: &Path) -> bool {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    path.starts_with(root)
}

fn path_to_url(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            std::path::Component::Normal(os) => Some(os.to_string_lossy().to_string()),
            std::path::Component::CurDir => None,
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

async fn serve_preview(
    output: PathBuf,
    version: Arc<AtomicU64>,
    listener: tokio::net::TcpListener,
) -> Result<()> {
    use axum::{routing::get, Router};
    use tower_http::services::ServeDir;

    let state = Arc::new(PreviewState { version });
    let app = Router::new()
        .route("/__rendar_version", get(version_handler))
        .nest_service("/", ServeDir::new(output).append_index_html_on_directories(true))
        .with_state(state);

    axum::serve(listener, app).await.context("Preview server failed")
}

#[derive(Clone)]
struct PreviewState {
    version: Arc<AtomicU64>,
}

async fn version_handler(State(state): State<Arc<PreviewState>>) -> String {
    state.version.load(Ordering::SeqCst).to_string()
}

fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("open");

    #[cfg(target_os = "linux")]
    let mut command = std::process::Command::new("xdg-open");

    #[cfg(target_os = "windows")]
    let mut command = {
        let mut cmd = std::process::Command::new("cmd");
        cmd.arg("/C").arg("start");
        cmd
    };

    command.arg(url);
    let _ = command.spawn();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, PreviewConfig};

    #[test]
    fn resolves_input_with_cli_override() {
        let config = Config {
            input: Some(PathBuf::from("config-input")),
            template: None,
            exclude: None,
            preview: None,
        };
        let resolved = resolve_input(Some(PathBuf::from("cli-input")), Some(&config));
        assert_eq!(resolved, PathBuf::from("cli-input"));
    }

    #[test]
    fn resolves_template_with_config_fallback() {
        let config = Config {
            input: None,
            template: Some(PathBuf::from("config-template.html")),
            exclude: None,
            preview: None,
        };
        let resolved = resolve_template(None, Some(&config));
        assert_eq!(resolved, Some(PathBuf::from("config-template.html")));
    }

    #[test]
    fn resolves_preview_port_with_cli_override() {
        let config = Config {
            input: None,
            template: None,
            exclude: None,
            preview: Some(PreviewConfig {
                port: Some(4000),
                open: None,
            }),
        };
        let resolved = resolve_preview_port(Some(5000), Some(&config));
        assert_eq!(resolved, 5000);
    }

    #[test]
    fn resolves_preview_open_with_config_fallback() {
        let config = Config {
            input: None,
            template: None,
            exclude: None,
            preview: Some(PreviewConfig {
                port: None,
                open: Some(true),
            }),
        };
        let resolved = resolve_preview_open(false, Some(&config));
        assert!(resolved);
    }

    #[test]
    fn resolves_preview_port_default_when_unset() {
        let config = Config {
            input: None,
            template: None,
            exclude: None,
            preview: None,
        };
        let resolved = resolve_preview_port(None, Some(&config));
        assert_eq!(resolved, 3000);
    }

    #[test]
    fn resolves_start_page_from_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("README.md"), "# Readme").expect("readme");
        let resolved = resolve_start_page(dir.path()).expect("resolve start");
        assert!(resolved.ends_with("README.md"));
    }

    #[test]
    fn resolves_preview_paths_with_input_override() {
        let root = tempfile::tempdir().expect("tempdir");
        let root_path = root.path();
        let docs = root_path.join("docs");
        std::fs::create_dir_all(&docs).expect("docs dir");
        let page = docs.join("page.md");
        std::fs::write(&page, "# Page").expect("page");

        let cwd = root_path.to_path_buf();
        let input_override = Some(root_path.to_path_buf());
        let start_on = Some(page.clone());
        let paths = resolve_preview_paths_with_cwd(&cwd, input_override, start_on)
            .expect("preview paths");
        assert_eq!(paths.input_root, root_path);
        assert_eq!(paths.start_page.unwrap(), page);
    }

    #[test]
    fn discovers_root_from_start_page() {
        let root = tempfile::tempdir().expect("tempdir");
        let root_path = root.path();
        let docs = root_path.join("docs");
        std::fs::create_dir_all(&docs).expect("docs dir");
        std::fs::write(docs.join("README.md"), "# Docs").expect("docs readme");
        let nested = docs.join("nested");
        std::fs::create_dir_all(&nested).expect("nested dir");
        let page = nested.join("page.md");
        std::fs::write(&page, "# Page").expect("page");

        let discovered = discover_root_for_start(&page);
        assert_eq!(discovered, root_path.join("docs"));
    }

    #[test]
    fn errors_when_start_page_outside_input() {
        let root = tempfile::tempdir().expect("tempdir");
        let input_root = root.path().join("input");
        let other_root = root.path().join("other");
        std::fs::create_dir_all(&input_root).expect("input dir");
        std::fs::create_dir_all(&other_root).expect("other dir");
        let page = other_root.join("page.md");
        std::fs::write(&page, "# Page").expect("page");

        let cwd = root.path().to_path_buf();
        let input_override = Some(input_root);
        let start_on = Some(page);
        let result = resolve_preview_paths_with_cwd(&cwd, input_override, start_on);
        assert!(result.is_err());
    }
}
