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
    let input = input
        .or_else(|| config.as_ref().and_then(|cfg| cfg.input.clone()))
        .unwrap_or_else(|| PathBuf::from("."));
    let template = template.or_else(|| config.as_ref().and_then(|cfg| cfg.template.clone()));
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

fn run_preview(
    input: Option<PathBuf>,
    config: Option<PathBuf>,
    template: Option<PathBuf>,
    open: bool,
    port: Option<u16>,
) -> Result<()> {
    let config = config::load_config(config.as_deref())?;
    let input = input
        .or_else(|| config.as_ref().and_then(|cfg| cfg.input.clone()))
        .unwrap_or_else(|| PathBuf::from("."));
    let template = template.or_else(|| config.as_ref().and_then(|cfg| cfg.template.clone()));
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

    let port = port
        .or_else(|| {
            config
                .as_ref()
                .and_then(|cfg| cfg.preview.as_ref())
                .and_then(|preview| preview.port)
        })
        .unwrap_or(3000);
    let address = format!("127.0.0.1:{}", port);
    println!("Preview server running at http://{address}");
    let open = if open {
        true
    } else {
        config
            .as_ref()
            .and_then(|cfg| cfg.preview.as_ref())
            .and_then(|preview| preview.open)
            .unwrap_or(false)
    };
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
