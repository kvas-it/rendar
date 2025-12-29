use anyhow::{Context, Result};
use axum::extract::State;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
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
    },
    /// Check for broken links and other warnings without writing output.
    Check {
        /// Input directory to scan (defaults to current directory).
        #[arg(short, long)]
        input: Option<PathBuf>,
        /// Optional config file path (e.g., rendar.toml).
        #[arg(short, long)]
        config: Option<PathBuf>,
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
        /// Open the browser after starting the server.
        #[arg(long)]
        open: bool,
        /// Port for the preview server.
        #[arg(long)]
        port: Option<u16>,
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
        } => run_build(out, input, config, template),
        Command::Check { input, config } => run_check(input, config),
        Command::Preview {
            input,
            config,
            template,
            open,
            port,
        } => run_preview(input, config, template, open, port),
    }
}

fn run_build(
    out: PathBuf,
    input: Option<PathBuf>,
    config: Option<PathBuf>,
    template: Option<PathBuf>,
) -> Result<()> {
    let config = config::load_config(config.as_deref())?;
    let input = resolve_input(input, config.as_ref());
    let template = resolve_template(template, config.as_ref());
    let template = load_template(template)?;
    site::build_site(
        &input,
        &out,
        &site::RenderOptions {
            live_reload: false,
            template: &template,
        },
    )?;
    println!("Rendered site to {}", out.display());
    Ok(())
}

fn run_check(input: Option<PathBuf>, config: Option<PathBuf>) -> Result<()> {
    let config = config::load_config(config.as_deref())?;
    let input = resolve_input(input, config.as_ref());
    let warnings = site::check_site(&input)?;
    if warnings > 0 {
        std::process::exit(1);
    }
    Ok(())
}

fn run_preview(
    input: Option<PathBuf>,
    config: Option<PathBuf>,
    template: Option<PathBuf>,
    open: bool,
    port: Option<u16>,
) -> Result<()> {
    let config = config::load_config(config.as_deref())?;
    let input = resolve_input(input, config.as_ref());
    let template = resolve_template(template, config.as_ref());
    let template = load_template(template)?;
    let temp_dir = tempfile::tempdir().context("Failed to create preview directory")?;
    let output = temp_dir.path().to_path_buf();
    site::build_site(
        &input,
        &output,
        &site::RenderOptions {
            live_reload: true,
            template: &template,
        },
    )?;

    let version = Arc::new(AtomicU64::new(1));
    let watcher_version = Arc::clone(&version);
    let input_clone = input.clone();
    let output_clone = output.clone();

    std::thread::spawn(move || {
        if let Err(err) = watch_and_rebuild(
            &input_clone,
            &output_clone,
            watcher_version,
            template,
        ) {
            eprintln!("Preview watcher error: {err}");
        }
    });

    let port = resolve_preview_port(port, config.as_ref());
    let address = format!("127.0.0.1:{}", port);
    println!("Preview server running at http://{address}");
    let open = resolve_preview_open(open, config.as_ref());
    if open {
        open_browser(&format!("http://{address}"));
    }

    let rt = tokio::runtime::Runtime::new().context("Failed to start async runtime")?;
    rt.block_on(async move {
        serve_preview(output, version, &address).await
    })
}

fn watch_and_rebuild(
    input: &std::path::Path,
    output: &std::path::Path,
    version: Arc<AtomicU64>,
    template: template::Template,
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

async fn serve_preview(
    output: PathBuf,
    version: Arc<AtomicU64>,
    address: &str,
) -> Result<()> {
    use axum::{routing::get, Router};
    use tower_http::services::ServeDir;

    let state = Arc::new(PreviewState { version });
    let app = Router::new()
        .route("/__rendar_version", get(version_handler))
        .nest_service("/", ServeDir::new(output).append_index_html_on_directories(true))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(address)
        .await
        .context("Failed to bind preview server")?;
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
            preview: None,
        };
        let resolved = resolve_preview_port(None, Some(&config));
        assert_eq!(resolved, 3000);
    }
}
