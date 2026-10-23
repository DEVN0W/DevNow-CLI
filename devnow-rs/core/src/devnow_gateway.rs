use std::collections::VecDeque;
use std::env;
use std::ffi::OsString;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::Mutex as StdMutex;
use std::time::Duration;
use std::time::Instant;

use regex_lite::Regex;
use serde_json::json;
use tempfile::TempDir;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncRead;
use tokio::io::BufReader;
use tokio::process::Child;
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::error::CodexErr;
use crate::error::Result;

const GATEWAY_HOST: &str = "127.0.0.1";
const DEFAULT_UPSTREAM_BASE_URL: &str = "https://api.anthropic.com";
const DEFAULT_STARTUP_TIMEOUT_MS: u64 = 15_000;
const READY_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Managed gateway stderr should never be allowed to grow without bound: it is
/// only meant to provide a short diagnostic tail, not a full transcript.
const MAX_GATEWAY_LOG_LINES: usize = 100;
const MAX_GATEWAY_LOG_BYTES: usize = 64 * 1024;

/// How long to wait for the stderr collector to observe EOF before giving up
/// and using whatever tail it has accumulated so far. Bounded so a hung
/// child (or a pipe that never closes) can never block startup-failure or
/// shutdown reporting.
const COLLECTOR_DRAIN_WAIT: Duration = Duration::from_millis(300);

/// Bounded wait for the child process to exit during graceful shutdown,
/// before the collector is asked to finish up.
const SHUTDOWN_CHILD_WAIT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone)]
pub(crate) struct DevNowGatewayManager {
    state: Arc<Mutex<GatewayState>>,
    settings: GatewaySettings,
}

#[derive(Debug)]
enum GatewayState {
    Stopped,
    Ready(RunningGateway),
    Failed(String),
}

#[derive(Debug)]
struct RunningGateway {
    base_url: String,
    model: String,
    child: Child,
    log_collector: GatewayLogCollector,
    _runtime_dir: TempDir,
}

#[derive(Debug, Clone)]
struct GatewaySettings {
    python_executable: OsString,
    gateway_root: PathBuf,
    startup_timeout: Duration,
    upstream_base_url: String,
}

impl GatewaySettings {
    fn from_env() -> Self {
        let python_executable = env::var_os("DEVNOW_GATEWAY_PYTHON")
            .unwrap_or_else(|| OsString::from(default_python_executable()));
        let gateway_root = env::var_os("DEVNOW_GATEWAY_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(default_gateway_root);
        let startup_timeout = env::var("DEVNOW_GATEWAY_STARTUP_TIMEOUT_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .map(Duration::from_millis)
            .unwrap_or(Duration::from_millis(DEFAULT_STARTUP_TIMEOUT_MS));
        let upstream_base_url = env::var("DEVNOW_CLAUDE_BASE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_UPSTREAM_BASE_URL.to_string());
        Self {
            python_executable,
            gateway_root,
            startup_timeout,
            upstream_base_url,
        }
    }
}

impl DevNowGatewayManager {
    pub(crate) fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(GatewayState::Stopped)),
            settings: GatewaySettings::from_env(),
        }
    }

    pub(crate) async fn ensure_ready(&self, model: &str) -> Result<String> {
        let mut state = self.state.lock().await;

        if let GatewayState::Failed(message) = &*state {
            return Err(CodexErr::InvalidRequest(message.clone()));
        }

        if let GatewayState::Ready(running) = &mut *state
            && running.model == model
        {
            match running.child.try_wait()? {
                None => return Ok(running.base_url.clone()),
                Some(status) => {
                    // The previously-Ready child exited on its own. Don't
                    // silently reuse (or silently replace) a dead process:
                    // invalidate the Ready state and surface a diagnosable
                    // error through the normal error path.
                    let previous = std::mem::replace(&mut *state, GatewayState::Stopped);
                    let GatewayState::Ready(dead) = previous else {
                        unreachable!("state was just matched as Ready");
                    };
                    let diagnostic = dead.log_collector.finish(COLLECTOR_DRAIN_WAIT).await;
                    let message = format!(
                        "DevoNow Gateway exited unexpectedly: {}",
                        compose_failure_message(
                            &format!("process exited with status {status}"),
                            diagnostic.as_deref()
                        )
                    );
                    *state = GatewayState::Failed(message.clone());
                    return Err(CodexErr::InvalidRequest(message));
                }
            }
        }

        if matches!(&*state, GatewayState::Ready(_)) {
            let previous = std::mem::replace(&mut *state, GatewayState::Stopped);
            let GatewayState::Ready(running) = previous else {
                unreachable!("state was just matched as Ready");
            };
            shutdown_running_gateway(running).await;
        }

        match self.start(model).await {
            Ok(running) => {
                let base_url = running.base_url.clone();
                *state = GatewayState::Ready(running);
                Ok(base_url)
            }
            Err(error) => {
                let message = format!("DevoNow Gateway failed to start: {error}");
                *state = GatewayState::Failed(message.clone());
                Err(CodexErr::InvalidRequest(message))
            }
        }
    }

    /// Request a graceful shutdown of any running gateway child: terminate
    /// it, wait briefly for exit, and let the stderr collector drain to EOF
    /// (or be aborted if it doesn't finish in time). Safe to call even when
    /// no gateway is running.
    pub(crate) async fn shutdown(&self) {
        let mut state = self.state.lock().await;
        if let GatewayState::Ready(running) = std::mem::replace(&mut *state, GatewayState::Stopped)
        {
            shutdown_running_gateway(running).await;
        }
    }

    async fn start(&self, model: &str) -> std::result::Result<RunningGateway, String> {
        if !self.settings.gateway_root.join("pyproject.toml").is_file() {
            return Err(format!(
                "gateway root `{}` is missing; set DEVNOW_GATEWAY_ROOT",
                self.settings.gateway_root.display()
            ));
        }
        let claude_api_key = env::var("DEVNOW_CLAUDE_API_KEY")
            .ok()
            .filter(|value| !value.trim().is_empty());
        if claude_api_key.is_none() {
            return Err("DEVNOW_CLAUDE_API_KEY is not set".to_string());
        }

        let port = available_local_port()?;
        let runtime_dir = tempfile::Builder::new()
            .prefix("devnow-gateway-")
            .tempdir()
            .map_err(|error| format!("could not create runtime directory: {error}"))?;
        let config_path = runtime_dir.path().join("devnow-gateway-config.json");
        let config = gateway_config(model, port, &self.settings.upstream_base_url);
        std::fs::write(
            &config_path,
            serde_json::to_vec_pretty(&config)
                .map_err(|error| format!("could not serialize runtime config: {error}"))?,
        )
        .map_err(|error| format!("could not write runtime config: {error}"))?;

        let source_root = self.settings.gateway_root.join("src");
        let python_path = prepend_python_path(source_root)?;
        let mut child = Command::new(&self.settings.python_executable)
            .arg("-m")
            .arg("llm_rosetta.gateway")
            .arg("--config")
            .arg(&config_path)
            .arg("--no-banner")
            .arg("--log-level")
            .arg("warning")
            .current_dir(&self.settings.gateway_root)
            .env("PYTHONPATH", python_path)
            // Defense in depth: the Python side already honors NO_COLOR, but
            // setting it here means correctness never depends solely on
            // stderr's TTY detection.
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| {
                format!(
                    "could not launch `{}`: {error}",
                    self.settings.python_executable.to_string_lossy()
                )
            })?;

        // Take and start draining stderr immediately, before the readiness
        // loop below: the OS pipe buffer can fill and deadlock the child if
        // nothing reads it continuously.
        let Some(stderr) = child.stderr.take() else {
            let _ = child.start_kill();
            return Err("internal error: gateway child stderr was not piped".to_string());
        };
        let log_collector = GatewayLogCollector::spawn(stderr, claude_api_key);

        let health_url = format!("http://{GATEWAY_HOST}:{port}/health/ready");
        let base_url = format!("http://{GATEWAY_HOST}:{port}/v1");
        let client = reqwest::Client::new();
        let deadline = Instant::now() + self.settings.startup_timeout;
        loop {
            if let Some(status) = child
                .try_wait()
                .map_err(|error| format!("could not inspect gateway process: {error}"))?
            {
                let diagnostic = log_collector.finish(COLLECTOR_DRAIN_WAIT).await;
                return Err(compose_failure_message(
                    &format!("process exited with status {status}"),
                    diagnostic.as_deref(),
                ));
            }
            if client
                .get(&health_url)
                .timeout(Duration::from_millis(500))
                .send()
                .await
                .is_ok_and(|response| response.status().is_success())
            {
                tracing::info!(port, pid = child.id(), "DevoNow Gateway is ready");
                return Ok(RunningGateway {
                    base_url,
                    model: model.to_string(),
                    child,
                    log_collector,
                    _runtime_dir: runtime_dir,
                });
            }
            if Instant::now() >= deadline {
                let _ = child.start_kill();
                let _ = tokio::time::timeout(SHUTDOWN_CHILD_WAIT, child.wait()).await;
                let diagnostic = log_collector.finish(COLLECTOR_DRAIN_WAIT).await;
                return Err(compose_failure_message(
                    &format!(
                        "readiness check timed out after {} ms",
                        self.settings.startup_timeout.as_millis()
                    ),
                    diagnostic.as_deref(),
                ));
            }
            tokio::time::sleep(READY_POLL_INTERVAL).await;
        }
    }
}

async fn shutdown_running_gateway(mut running: RunningGateway) {
    let _ = running.child.start_kill();
    let _ = tokio::time::timeout(SHUTDOWN_CHILD_WAIT, running.child.wait()).await;
    let _ = running.log_collector.finish(COLLECTOR_DRAIN_WAIT).await;
    // `running._runtime_dir` is dropped here, cleaning up the temp config dir.
}

/// Combine a short human-readable cause with a bounded, sanitized stderr
/// excerpt (when one is available) into a single diagnosable message.
fn compose_failure_message(cause: &str, diagnostic: Option<&str>) -> String {
    match diagnostic {
        Some(diagnostic) if !diagnostic.is_empty() => {
            format!("{cause}.\nGateway diagnostic: {diagnostic}")
        }
        _ => format!("{cause}."),
    }
}

fn default_python_executable() -> &'static str {
    if cfg!(windows) { "python" } else { "python3" }
}

fn default_gateway_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("third_party")
        .join("devnow-gateway")
}

fn available_local_port() -> std::result::Result<u16, String> {
    TcpListener::bind((GATEWAY_HOST, 0))
        .and_then(|listener| listener.local_addr())
        .map(|address| address.port())
        .map_err(|error| format!("could not allocate a localhost port: {error}"))
}

fn prepend_python_path(source_root: PathBuf) -> std::result::Result<OsString, String> {
    let mut paths = vec![source_root];
    if let Some(existing) = env::var_os("PYTHONPATH") {
        paths.extend(env::split_paths(&existing));
    }
    env::join_paths(paths).map_err(|error| format!("could not construct PYTHONPATH: {error}"))
}

fn gateway_config(model: &str, port: u16, upstream_base_url: &str) -> serde_json::Value {
    json!({
        "providers": {
            "anthropic": {
                "type": "anthropic",
                "api_key": "${DEVNOW_CLAUDE_API_KEY}",
                "base_url": upstream_base_url,
            },
        },
        "models": {
            model: "anthropic",
        },
        "server": {
            "host": GATEWAY_HOST,
            "port": port,
            "credential_visible": false,
        },
    })
}

// ---------------------------------------------------------------------------
// Gateway stderr capture: bounded tail, sanitization, and structured-log
// forwarding for the managed child process's stderr stream.
//
// DevNow owns all output from the managed gateway process. The child must
// never inherit the TUI's stdout/stderr handles; instead its stderr is
// piped and continuously drained here so the OS pipe buffer can never fill
// and deadlock the child. Records are sanitized (ANSI stripped, control
// characters normalized, secrets redacted) before being retained in a
// bounded tail or forwarded through `tracing`.
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
struct GatewayLogTail {
    lines: VecDeque<String>,
    byte_count: usize,
}

impl GatewayLogTail {
    fn push(&mut self, line: String) {
        self.byte_count += line.len();
        self.lines.push_back(line);
        while self.lines.len() > MAX_GATEWAY_LOG_LINES || self.byte_count > MAX_GATEWAY_LOG_BYTES {
            let Some(evicted) = self.lines.pop_front() else {
                break;
            };
            self.byte_count = self.byte_count.saturating_sub(evicted.len());
        }
    }

    fn excerpt(&self) -> Option<String> {
        if self.lines.is_empty() {
            None
        } else {
            Some(self.lines.iter().cloned().collect::<Vec<_>>().join("\n"))
        }
    }
}

#[derive(Debug)]
struct GatewayLogCollector {
    tail: Arc<StdMutex<GatewayLogTail>>,
    task: JoinHandle<()>,
}

impl GatewayLogCollector {
    fn spawn<R>(reader: R, secret_value: Option<String>) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
    {
        let tail = Arc::new(StdMutex::new(GatewayLogTail::default()));
        let task_tail = Arc::clone(&tail);
        let task = tokio::spawn(async move {
            drain_gateway_stderr(reader, &task_tail, secret_value.as_deref()).await;
        });
        Self { tail, task }
    }

    /// Wait up to `bound` for the collector to observe EOF, aborting the
    /// task if it hasn't finished by then so it can never leak or block
    /// child-process teardown. Returns a bounded, sanitized diagnostic
    /// excerpt of whatever was captured, if anything.
    async fn finish(self, bound: Duration) -> Option<String> {
        let Self { tail, mut task } = self;
        if tokio::time::timeout(bound, &mut task).await.is_err() {
            task.abort();
        }
        tail_excerpt(&tail)
    }
}

fn tail_excerpt(tail: &StdMutex<GatewayLogTail>) -> Option<String> {
    let guard = tail
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    guard.excerpt()
}

async fn drain_gateway_stderr<R>(
    reader: R,
    tail: &StdMutex<GatewayLogTail>,
    secret_value: Option<&str>,
) where
    R: AsyncRead + Unpin,
{
    let mut lines = BufReader::new(reader).lines();
    loop {
        match lines.next_line().await {
            Ok(Some(raw_line)) => {
                let sanitized = sanitize_gateway_log_line(&raw_line, secret_value);
                if sanitized.trim().is_empty() {
                    continue;
                }
                forward_gateway_log_line(&sanitized);
                let mut guard = tail
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                guard.push(sanitized);
            }
            // `next_line` returns the trailing partial line (if any) before
            // finally returning `Ok(None)` at true EOF, so no data is lost
            // when the child doesn't terminate its final line with `\n`.
            Ok(None) => break,
            Err(_) => break,
        }
    }
}

/// Route a sanitized gateway stderr line through `tracing` rather than
/// writing raw child bytes to the terminal. The Python formatter prefixes
/// records with a fixed-width level name (see `LEVEL_NAMES` in
/// `gateway/logging.py`), so warnings/errors are promoted accordingly;
/// everything else is kept at debug level.
fn forward_gateway_log_line(line: &str) {
    if line.contains("CRITICAL") || line.contains("ERROR") {
        tracing::error!(target: "devnow_gateway", "{line}");
    } else if line.contains("WARNING") {
        tracing::warn!(target: "devnow_gateway", "{line}");
    } else {
        tracing::debug!(target: "devnow_gateway", "{line}");
    }
}

fn sanitize_gateway_log_line(raw_line: &str, secret_value: Option<&str>) -> String {
    let stripped = strip_ansi_sequences(raw_line);
    let normalized = normalize_control_chars(&stripped);
    let mut redacted = redact_gateway_secrets(&normalized, secret_value);
    if redacted.len() > MAX_GATEWAY_LOG_BYTES {
        let prefix = "…";
        let retained_bytes = MAX_GATEWAY_LOG_BYTES.saturating_sub(prefix.len());
        let mut start = redacted.len() - retained_bytes;
        while !redacted.is_char_boundary(start) {
            start += 1;
        }
        redacted = format!("{prefix}{}", &redacted[start..]);
    }
    redacted
}

/// Defensively strip ANSI CSI (`ESC [ ... final-byte`) and OSC
/// (`ESC ] ... BEL` or `ESC ] ... ESC \`) sequences, plus any other
/// two-byte escape sequence, so terminal-control bytes never reach
/// downstream log renderers.
fn strip_ansi_sequences(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\u{1b}' {
            out.push(ch);
            continue;
        }
        match chars.peek() {
            Some('[') => {
                chars.next();
                for c in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&c) {
                        break;
                    }
                }
            }
            Some(']') => {
                chars.next();
                loop {
                    match chars.next() {
                        None => break,
                        Some('\u{07}') => break,
                        Some('\u{1b}') => {
                            if chars.peek() == Some(&'\\') {
                                chars.next();
                            }
                            break;
                        }
                        Some(_) => {}
                    }
                }
            }
            Some(_) => {
                // Generic two-byte escape sequence (e.g. cursor save/restore).
                chars.next();
            }
            None => {}
        }
    }
    out
}

/// Keep tabs and spaces; drop every other ASCII/Unicode control character
/// (e.g. `\r`, `\0`, other non-printable bytes) that could otherwise corrupt
/// a downstream terminal or log renderer.
fn normalize_control_chars(input: &str) -> String {
    input
        .chars()
        .filter(|&c| c == '\t' || c == ' ' || !c.is_control())
        .collect()
}

static ANTHROPIC_KEY_REGEX: LazyLock<Regex> =
    LazyLock::new(|| compile_regex(r"sk-ant-[A-Za-z0-9_\-]{10,}"));
static AUTHORIZATION_HEADER_REGEX: LazyLock<Regex> =
    LazyLock::new(|| compile_regex(r"(?i)(authorization\s*:\s*)\S.*"));
static X_API_KEY_HEADER_REGEX: LazyLock<Regex> =
    LazyLock::new(|| compile_regex(r"(?i)(x-api-key\s*:\s*)\S.*"));

fn compile_regex(pattern: &str) -> Regex {
    match Regex::new(pattern) {
        Ok(regex) => regex,
        Err(err) => panic!("invalid regex pattern `{pattern}`: {err}"),
    }
}

/// Redact bearer tokens, `x-api-key`/`Authorization` header values, and
/// Anthropic/OpenAI API-key-shaped strings before a line is retained or
/// forwarded. Also redacts the literal value of `DEVNOW_CLAUDE_API_KEY`
/// (passed in as `secret_value`) if it appears verbatim in the line.
fn redact_gateway_secrets(line: &str, secret_value: Option<&str>) -> String {
    let mut redacted = codex_secrets::redact_secrets(line.to_string());
    redacted = ANTHROPIC_KEY_REGEX
        .replace_all(&redacted, "[REDACTED_SECRET]")
        .into_owned();
    redacted = AUTHORIZATION_HEADER_REGEX
        .replace_all(&redacted, "$1[REDACTED_SECRET]")
        .into_owned();
    redacted = X_API_KEY_HEADER_REGEX
        .replace_all(&redacted, "$1[REDACTED_SECRET]")
        .into_owned();
    if let Some(secret) = secret_value
        && !secret.is_empty()
    {
        redacted = redacted.replace(secret, "[REDACTED_SECRET]");
    }
    redacted
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn generated_config_preserves_responses_to_anthropic_route() {
        let config = gateway_config(
            "claude-sonnet-4-20250514",
            43210,
            "https://api.anthropic.com",
        );
        assert_eq!(config["server"]["host"], GATEWAY_HOST);
        assert_eq!(config["server"]["port"], 43210);
        assert_eq!(config["models"]["claude-sonnet-4-20250514"], "anthropic");
        assert_eq!(
            config["providers"]["anthropic"]["api_key"],
            "${DEVNOW_CLAUDE_API_KEY}"
        );
    }

    #[tokio::test]
    async fn startup_failure_is_clear_and_sticky() {
        let manager = DevNowGatewayManager {
            state: Arc::new(Mutex::new(GatewayState::Stopped)),
            settings: GatewaySettings {
                python_executable: OsString::from("missing-python"),
                gateway_root: PathBuf::from("missing-gateway-root"),
                startup_timeout: Duration::from_millis(1),
                upstream_base_url: DEFAULT_UPSTREAM_BASE_URL.to_string(),
            },
        };

        let first = manager
            .ensure_ready("claude-sonnet-4-20250514")
            .await
            .expect_err("startup should fail");
        let second = manager
            .ensure_ready("claude-sonnet-4-20250514")
            .await
            .expect_err("failed startup should remain failed");
        assert_eq!(first.to_string(), second.to_string());
        assert!(
            first
                .to_string()
                .contains("DevoNow Gateway failed to start")
        );
    }

    /// A `tokio::io::AsyncRead` that yields fixed byte chunks in order and
    /// then reports EOF, letting tests exercise the collector's drain loop
    /// (including a trailing partial line) without spawning a real process.
    struct ChunkedReader {
        chunks: VecDeque<Vec<u8>>,
    }

    impl ChunkedReader {
        fn new(chunks: Vec<&[u8]>) -> Self {
            Self {
                chunks: chunks.into_iter().map(<[u8]>::to_vec).collect(),
            }
        }
    }

    impl AsyncRead for ChunkedReader {
        fn poll_read(
            mut self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            if let Some(chunk) = self.chunks.pop_front() {
                buf.put_slice(&chunk);
            }
            std::task::Poll::Ready(Ok(()))
        }
    }

    async fn collect_tail(chunks: Vec<&[u8]>) -> Vec<String> {
        let reader = ChunkedReader::new(chunks);
        let collector = GatewayLogCollector::spawn(reader, None);
        // Drive the collector to completion by waiting for it directly.
        let tail = Arc::clone(&collector.tail);
        if collector.task.await.is_err() {
            panic!("collector task should not panic");
        }
        let guard = tail
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.lines.iter().cloned().collect()
    }

    #[tokio::test]
    async fn collector_drains_multiple_complete_lines() {
        let lines = collect_tail(vec![b"first line\nsecond line\nthird line\n"]).await;
        assert_eq!(lines, vec!["first line", "second line", "third line"]);
    }

    #[tokio::test]
    async fn collector_handles_final_partial_line_at_eof() {
        let lines = collect_tail(vec![b"complete line\n", b"trailing without newline"]).await;
        assert_eq!(lines, vec!["complete line", "trailing without newline"]);
    }

    #[test]
    fn ansi_csi_and_osc_sequences_are_stripped() {
        let csi = "\u{1b}[31mred text\u{1b}[0m plain";
        assert_eq!(strip_ansi_sequences(csi), "red text plain");

        let osc_bel = "\u{1b}]0;window title\u{07}after";
        assert_eq!(strip_ansi_sequences(osc_bel), "after");

        let osc_st = "\u{1b}]0;window title\u{1b}\\after";
        assert_eq!(strip_ansi_sequences(osc_st), "after");
    }

    #[test]
    fn control_characters_are_normalized_but_tabs_and_spaces_survive() {
        let input = "keep\ttabs and spaces\u{0007}\u{0000}bell-and-null-dropped";
        assert_eq!(
            normalize_control_chars(input),
            "keep\ttabs and spacesbell-and-null-dropped"
        );
    }

    #[test]
    fn bearer_token_shaped_values_are_redacted() {
        let line = "auth header: Bearer abcdefghijklmnopqrstuvwxyz0123456789";
        let redacted = redact_gateway_secrets(line, None);
        assert!(!redacted.contains("abcdefghijklmnopqrstuvwxyz"));
        assert!(redacted.contains("[REDACTED_SECRET]"));
    }

    #[test]
    fn anthropic_key_shaped_values_are_redacted_even_without_a_keyword_prefix() {
        // No "Bearer"/"api_key"/"token" keyword precedes this, so only the
        // Anthropic-specific key-shape regex should catch it.
        let line = "using key sk-ant-api03-abcdefghijklmnopqrstuvwxyz012345 failed";
        let redacted = redact_gateway_secrets(line, None);
        assert!(!redacted.contains("abcdefghijklmnopqrstuvwxyz"));
        assert!(redacted.contains("[REDACTED_SECRET]"));
    }

    #[test]
    fn x_api_key_and_authorization_headers_are_redacted() {
        let line = "x-api-key: super-secret-value-123";
        assert_eq!(
            redact_gateway_secrets(line, None),
            "x-api-key: [REDACTED_SECRET]"
        );

        let line = "Authorization: Bearer abcdefghijklmnopqrstuvwx";
        let redacted = redact_gateway_secrets(line, None);
        assert!(!redacted.contains("abcdefghijklmnopqrstuvwx"));
    }

    #[test]
    fn configured_secret_env_value_is_redacted_verbatim() {
        let secret = "sk-live-my-devnow-claude-key";
        let line = format!("failed to authenticate using {secret}");
        let redacted = redact_gateway_secrets(&line, Some(secret));
        assert_eq!(redacted, "failed to authenticate using [REDACTED_SECRET]");
    }

    #[test]
    fn line_and_byte_limits_evict_oldest_content_first() {
        let mut tail = GatewayLogTail::default();
        for i in 0..(MAX_GATEWAY_LOG_LINES + 10) {
            tail.push(format!("line-{i}"));
        }
        assert_eq!(tail.lines.len(), MAX_GATEWAY_LOG_LINES);
        // The oldest 10 lines (line-0..line-9) should have been evicted.
        assert_eq!(tail.lines.front().cloned(), Some("line-10".to_string()));
        assert_eq!(
            tail.lines.back().cloned(),
            Some(format!("line-{}", MAX_GATEWAY_LOG_LINES + 9))
        );

        let mut byte_tail = GatewayLogTail::default();
        let big_line = "x".repeat(MAX_GATEWAY_LOG_BYTES / 4);
        for _ in 0..8 {
            byte_tail.push(big_line.clone());
        }
        assert!(byte_tail.byte_count <= MAX_GATEWAY_LOG_BYTES);
        assert!(byte_tail.lines.len() < 8);
    }

    #[test]
    fn diagnostic_excerpt_remains_bounded() {
        let mut tail = GatewayLogTail::default();
        for i in 0..(MAX_GATEWAY_LOG_LINES * 2) {
            tail.push(format!("diagnostic line {i}"));
        }
        let excerpt = tail.excerpt().expect("tail should not be empty");
        assert!(excerpt.lines().count() <= MAX_GATEWAY_LOG_LINES);
        assert!(excerpt.len() <= MAX_GATEWAY_LOG_BYTES);
    }

    #[tokio::test]
    async fn shutdown_completes_without_leaving_a_blocked_collector_task() {
        // A reader that never returns EOF on its own: the collector task
        // would run forever unless `finish` aborts it within the bound.
        struct NeverEndingReader;
        impl AsyncRead for NeverEndingReader {
            fn poll_read(
                self: std::pin::Pin<&mut Self>,
                cx: &mut std::task::Context<'_>,
                _buf: &mut tokio::io::ReadBuf<'_>,
            ) -> std::task::Poll<std::io::Result<()>> {
                // Always pending: simulates a pipe that is never closed.
                cx.waker().wake_by_ref();
                std::task::Poll::Pending
            }
        }

        let collector = GatewayLogCollector::spawn(NeverEndingReader, None);
        let task_handle_finished = tokio::time::timeout(
            Duration::from_secs(5),
            collector.finish(Duration::from_millis(50)),
        )
        .await;

        assert!(
            task_handle_finished.is_ok(),
            "collector.finish() must return promptly by aborting the stuck task"
        );
    }

    #[tokio::test]
    async fn startup_failure_with_real_short_lived_child_includes_sanitized_excerpt() {
        let Some(python) = find_python_executable() else {
            eprintln!("skipping: no python3/python executable found on PATH");
            return;
        };

        // Exercise the collector + child-exit diagnostic path directly with
        // a small script that mimics a real Python startup failure on
        // stderr, mirroring what `start()` does after `spawn()`.
        let mut child = tokio::process::Command::new(python)
            .arg("-c")
            .arg("import sys; sys.stderr.write('ModuleNotFoundError: no such module\\n'); sys.exit(1)")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap_or_else(|error| panic!("failed to spawn python child: {error}"));

        let Some(stderr) = child.stderr.take() else {
            panic!("piped stderr was not available");
        };
        let collector = GatewayLogCollector::spawn(stderr, None);
        let status = child.wait().await.expect("child should exit");
        assert!(!status.success());

        let diagnostic = collector.finish(COLLECTOR_DRAIN_WAIT).await;
        let message = compose_failure_message(
            &format!("process exited with status {status}"),
            diagnostic.as_deref(),
        );

        assert!(
            message.contains("ModuleNotFoundError"),
            "expected sanitized excerpt in message, got: {message}"
        );
        assert!(message.contains("Gateway diagnostic:"));
        assert_ne!(
            message.trim(),
            format!("process exited with status {status}.")
        );
    }

    #[tokio::test]
    async fn manager_shutdown_terminates_running_child_and_resets_state() {
        let Some(python) = find_python_executable() else {
            eprintln!("skipping: no python3/python executable found on PATH");
            return;
        };

        let mut child = tokio::process::Command::new(python)
            .arg("-c")
            .arg("import time; time.sleep(30)")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap_or_else(|error| panic!("failed to spawn python child: {error}"));
        let Some(stderr) = child.stderr.take() else {
            panic!("piped stderr was not available");
        };
        let log_collector = GatewayLogCollector::spawn(stderr, None);
        let runtime_dir = tempfile::Builder::new()
            .prefix("devnow-gateway-shutdown-test-")
            .tempdir()
            .unwrap_or_else(|error| panic!("failed to create temp dir: {error}"));

        let manager = DevNowGatewayManager {
            state: Arc::new(Mutex::new(GatewayState::Ready(RunningGateway {
                base_url: "http://127.0.0.1:0/v1".to_string(),
                model: "claude-sonnet-4-20250514".to_string(),
                child,
                log_collector,
                _runtime_dir: runtime_dir,
            }))),
            settings: GatewaySettings {
                python_executable: OsString::from(python),
                gateway_root: default_gateway_root(),
                startup_timeout: Duration::from_millis(500),
                upstream_base_url: DEFAULT_UPSTREAM_BASE_URL.to_string(),
            },
        };

        // A long-sleeping child would hang shutdown forever if termination,
        // the exit wait, and the collector drain weren't all bounded.
        let result = tokio::time::timeout(Duration::from_secs(5), manager.shutdown()).await;
        assert!(
            result.is_ok(),
            "shutdown must complete within the bounded timeout"
        );

        let state = manager.state.lock().await;
        assert!(matches!(&*state, GatewayState::Stopped));
    }

    fn find_python_executable() -> Option<&'static str> {
        ["python3", "python"]
            .into_iter()
            .find(|&candidate| {
                std::process::Command::new(candidate)
                    .arg("--version")
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .is_ok_and(|status| status.success())
            })
            .map(|v| v as _)
    }
}
