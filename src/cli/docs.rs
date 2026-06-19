// Jetporch
// Copyright (C) 2023 - Michael DeHaan <michael@michaeldehaan.net> + contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <http://www.gnu.org/licenses/>.

// `jetpack docs` — build and serve the Hugo/Hextra documentation site locally,
// with live reload. Tries the local Hugo toolchain first and falls back to a
// containerised Hugo (hugomods/hugo:exts) when it is missing or unable to build
// the Hextra asset pipeline. The docs always reflect the current working tree.

use crate::cli::parser::CliParser;
use crate::util::terminal::banner;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Docker image used for the fallback serve path. Bundles Hugo extended + Go +
/// Node + PostCSS, so it can build Hextra even on a host with no JS toolchain.
const DOCS_IMAGE: &str = "hugomods/hugo:exts";

/// Entry point invoked from main.rs. Errors are printed to stderr and mapped to
/// exit code 1; a running server returns the child's own exit code.
pub fn docs(parser: &CliParser) -> i32 {
    match docs_inner(parser) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{}", e);
            1
        }
    }
}

fn docs_inner(parser: &CliParser) -> Result<i32, String> {
    let docs_root = find_docs_root()?;

    if local_hugo_available() {
        match validate_local_build(&docs_root) {
            BuildOutcome::Ok => return serve_local(&docs_root, parser),
            BuildOutcome::ToolchainGap(reason) => {
                banner(&format!(
                    "local Hugo built the site but its asset pipeline failed ({});",
                    reason
                ));
                banner("falling back to Docker…");
                return serve_docker(&docs_root, parser);
            }
            BuildOutcome::ContentError(err) => {
                // A genuine docs/config error — surface it verbatim, never mask
                // it with a Docker detour.
                return Err(err);
            }
        }
    }

    banner("Hugo (or Go) was not found locally; serving with Docker…");
    serve_docker(&docs_root, parser)
}

// -------------------------------------------------------------------------------------------------
// Site discovery
// -------------------------------------------------------------------------------------------------

/// Locate the docs site: $JET_DOCS_DIR if set and valid, else walk up from the
/// current directory looking for a `docs/` containing both hugo.yaml and go.mod.
fn find_docs_root() -> Result<PathBuf, String> {
    if let Ok(explicit) = std::env::var("JET_DOCS_DIR") {
        let candidate = PathBuf::from(&explicit);
        if site_is_at(&candidate) {
            return Ok(candidate);
        }
        return Err(format!(
            "$JET_DOCS_DIR ({}) does not look like the jetpack docs site \
             (expected hugo.yaml + go.mod)",
            candidate.display()
        ));
    }
    let mut dir = std::env::current_dir()
        .map_err(|e| format!("could not determine the current directory: {}", e))?;
    for _ in 0..8 {
        let candidate = dir.join("docs");
        if site_is_at(&candidate) {
            return Ok(candidate);
        }
        if !dir.pop() {
            break;
        }
    }
    Err(String::from(
        "could not locate the jetpack docs site (expected docs/hugo.yaml + docs/go.mod \
         in this or a parent directory); run from the repo root or set $JET_DOCS_DIR",
    ))
}

fn site_is_at(dir: &Path) -> bool {
    dir.join("hugo.yaml").is_file() && dir.join("go.mod").is_file()
}

// -------------------------------------------------------------------------------------------------
// Local toolchain + validation probe
// -------------------------------------------------------------------------------------------------

fn local_hugo_available() -> bool {
    command_ok("hugo", &["version"]) && command_ok("go", &["version"])
}

fn command_ok(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

enum BuildOutcome {
    Ok,
    /// The toolchain is present but a missing external tool (PostCSS, a Hugo
    /// module, etc.) blocked the build — Docker can help.
    ToolchainGap(String),
    /// The site itself is broken (bad front matter, template error, etc.) —
    /// Docker will not help; surface the real error.
    ContentError(String),
}

/// Run a one-shot, bounded Hugo build to memory (writes nothing to disk) to
/// decide whether `hugo server` will succeed locally or needs the Docker path.
fn validate_local_build(docs_root: &Path) -> BuildOutcome {
    let output = Command::new("hugo")
        .arg("--source")
        .arg(docs_root)
        .arg("--renderToMemory")
        .arg("--logLevel")
        .arg("error")
        .arg("--enableGitInfo=false")
        .output();

    let output = match output {
        Ok(o) => o,
        Err(e) => return BuildOutcome::ToolchainGap(format!("could not run hugo: {}", e)),
    };
    if output.status.success() {
        return BuildOutcome::Ok;
    }
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if is_toolchain_error(&combined) {
        BuildOutcome::ToolchainGap(first_line(&combined))
    } else {
        BuildOutcome::ContentError(combined.trim().to_string())
    }
}

/// Deliberately over-inclusive: anything that smells like a missing external
/// tool routes to Docker, while genuine content/config errors stay surfaced.
fn is_toolchain_error(s: &str) -> bool {
    let lower = s.to_lowercase();
    [
        "postcss",
        "tailwind",
        "esbuild",
        "node_modules",
        "cannot find module",
        "failed to load module",
        "hugo module",
        "go: ",
        "fork/exec",
        "no such file or directory",
        "command not found",
        "exec: ",
    ]
    .iter()
    .any(|t| lower.contains(t))
}

fn first_line(s: &str) -> String {
    s.lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim()
        .to_string()
}

// -------------------------------------------------------------------------------------------------
// Serving
// -------------------------------------------------------------------------------------------------

fn serve_local(docs_root: &Path, parser: &CliParser) -> Result<i32, String> {
    let port = pick_port(parser)?;
    let base_url = format!("http://127.0.0.1:{}/", port);

    let mut cmd = Command::new("hugo");
    cmd.arg("server")
        .arg("--source")
        .arg(docs_root)
        .arg("--baseURL")
        .arg(&base_url)
        .arg("--bind")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(port.to_string())
        .arg("--buildDrafts")
        .arg("--navigateToChanged")
        .arg("--disableFastRender")
        .arg("--noBuildLock");

    run_server(cmd, &base_url, parser)
}

fn serve_docker(docs_root: &Path, parser: &CliParser) -> Result<i32, String> {
    if !command_ok("docker", &["info"]) {
        return Err(String::from(
            "the documentation toolchain is unavailable: local Hugo is missing or \
             cannot build the site, and Docker is not installed or not running. Install \
             Hugo extended + Go, or start Docker.",
        ));
    }
    ensure_image()?;

    let port = pick_port(parser)?;
    let base_url = format!("http://127.0.0.1:{}/", port);
    let publish = format!("127.0.0.1:{}:1313", port);
    let mount = format!("{}:/src", docs_root.to_string_lossy());

    let mut cmd = Command::new("docker");
    cmd.arg("run")
        .arg("--rm")
        .arg("-i")
        .arg("-p")
        .arg(&publish)
        .arg("-v")
        .arg(&mount)
        .arg("-w")
        .arg("/src")
        .arg("-v")
        .arg("jetpack-hugo-modcache:/go/pkg/mod")
        .arg("-e")
        .arg("HUGO_CACHEDIR=/tmp/hugo_cache")
        .arg(DOCS_IMAGE)
        .arg("hugo")
        .arg("server")
        .arg("--bind")
        .arg("0.0.0.0")
        .arg("--port")
        .arg("1313")
        .arg("--buildDrafts")
        .arg("--navigateToChanged")
        .arg("--disableFastRender")
        .arg("--noBuildLock")
        .arg("--renderToMemory")
        .arg("--baseURL")
        .arg(&base_url);

    run_server(cmd, &base_url, parser)
}

fn ensure_image() -> Result<(), String> {
    let present = Command::new("docker")
        .args(["image", "inspect", DOCS_IMAGE, "--format", "{{.Id}}"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if present {
        return Ok(());
    }
    banner(&format!(
        "first run: pulling {} (bundles Hugo extended + Go + Node + PostCSS)…",
        DOCS_IMAGE
    ));
    let status = Command::new("docker")
        .arg("pull")
        .arg(DOCS_IMAGE)
        .status()
        .map_err(|e| format!("could not run docker pull: {}", e))?;
    if !status.success() {
        return Err(format!(
            "failed to pull {}; see the output above",
            DOCS_IMAGE
        ));
    }
    Ok(())
}

/// Spawn the server (stdio inherited so the user sees Hugo's output and Ctrl+C
/// propagates through the shared process group), open the browser unless
/// `--no-browser`, then block until the server exits.
fn run_server(mut cmd: Command, base_url: &str, parser: &CliParser) -> Result<i32, String> {
    banner(&format!(
        "serving jetpack docs at {}   (Ctrl+C to stop)",
        base_url
    ));
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to start the docs server: {}", e))?;
    if !parser.no_browser {
        open_browser(base_url);
    }
    let status = child
        .wait()
        .map_err(|e| format!("the docs server did not exit cleanly: {}", e))?;
    Ok(status.code().unwrap_or(1))
}

/// Pick the HTTP port: honour an explicit `--port`, otherwise let the OS choose
/// a free loopback port.
fn pick_port(parser: &CliParser) -> Result<u16, String> {
    if parser.port_set {
        let p = parser.default_port;
        if !(1..=65535).contains(&p) {
            return Err(format!("--port {} is out of range (1-65535)", p));
        }
        return Ok(p as u16);
    }
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .map_err(|e| format!("could not bind a local port: {}", e))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("could not resolve the local port: {}", e))?
        .port();
    drop(listener);
    Ok(port)
}

/// Open `url` in the user's browser. Fire-and-forget: a headless `xdg-open`
/// (e.g. over SSH) fails silently into null stdio.
fn open_browser(url: &str) {
    let (program, args): (&str, Vec<String>) = if cfg!(target_os = "macos") {
        ("open", vec![url.to_string()])
    } else if cfg!(target_os = "windows") {
        // the empty title arg is required: `start` treats a quoted first arg
        // as a window title otherwise.
        (
            "cmd",
            vec!["/C".into(), "start".into(), "".into(), url.to_string()],
        )
    } else {
        ("xdg-open", vec![url.to_string()])
    };
    let _ = Command::new(program)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}
