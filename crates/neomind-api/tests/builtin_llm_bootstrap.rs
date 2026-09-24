//! End-to-end integration of the builtin-model pipeline:
//! download → sha verify → manifest persist → spawn (fake llama-server on
//! PATH) → healthy → instance registered + capabilities recorded.
//!
//! Fakes: an axum server serves the "model file" (any bytes — nothing parses
//! it in-process); a shell script masquerades as `neomind-llama-server`
//! (serves /health and /props) injected via PATH, exercising the REAL
//! discovery, download-with-resume+sha, manifest, spawn, health-probe,
//! is_alive and registration code paths.
//!
//! Regression ground: the 2026-09 port move, source switch, quant tables and
//! lifecycle changes all landed with unit tests per link — this pins the
//! CHAIN.

// ENV_LOCK (std::sync::Mutex) is held across awaits by design: each test
// mutates process-global state (PATH) and each #[tokio::test] owns its own
// runtime, so the guard only ever blocks OS threads BETWEEN tests — it
// cannot deadlock inside one.
#![allow(clippy::await_holding_lock)]

use neomind_agent::llm_backends::LlmBackendInstanceManager;
use neomind_api::builtin_llm::state::{bootstrap, BootstrapOutcome, BUILTIN_INSTANCE_ID};
use neomind_storage::LlmBackendStore;
use std::io::Write;
use std::sync::Arc;

struct Fixture {
    data_dir: tempfile::TempDir,
    bin_dir: tempfile::TempDir,
    port: u16,
    manager: Arc<LlmBackendInstanceManager>,
}

impl Fixture {
    /// Build the fixture: PATH-injected fake llama-server + free port +
    /// instance manager backed by a temp store.
    fn new(server_port: u16, props_model_path: &str) -> Self {
        let data_dir = tempfile::tempdir().unwrap();
        let bin_dir = tempfile::tempdir().unwrap();

        // The fake llama-server: an HTTP /health + /props server bound to
        // server_port, staying alive until killed (kill_on_drop path uses
        // start_kill — a plain process works).
        let script = format!(
            r#"#!/bin/sh
_script_dir="$(cd "$(dirname "$0")" && pwd)"
echo "spawned: dir=$_script_dir args=$*" >> /tmp/fake-spawns.log
# FAST-FAIL gate, SHELL-level on purpose: the port-squat test needs our
# spawn to die in ~milliseconds so the health settle window in
# wait_healthy_loop_checking_child observes the exit. When this gate lived
# in python, a loaded machine could start python slower than the 500ms
# window — the child looked alive, bootstrap accepted the foreign
# squatter, and the test flapped.
if [ -f "$_script_dir/FAST_FAIL" ]; then
    exit 1
fi
exec python3 -c '
import http.server, socketserver, json, sys
class H(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/health":
            body = b"ok"
        elif self.path == "/props":
            body = json.dumps({{"model_path": "{model}", "n_ctx": 32768}}).encode()
        else:
            self.send_error(404); return
        self.send_response(200)
        self.send_header("Content-Type", "application/json" if self.path == "/props" else "text/plain")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    def log_message(self, *a): pass
# NOTE: deliberately NO allow_reuse_address — on macOS it lets a second
# bind to the same port SUCCEED silently (SO_REUSEADDR semantics), which
# would defeat the port-squat test where the squatter must WIN the bind.
# WARNING — keep this python payload free of APOSTROPHES. It rides inside
# a single-quoted `python3 -c` shell string, so one apostrophe anywhere
# (even in a comment) ends the string early: python receives a truncated
# program (imports + class def only), exits 0, and every spawned fake
# dies instantly. That exact bug masqueraded as sandbox flakiness for
# days — tests failed fast with "server unhealthy" while the same payload
# worked when run by hand with double quotes.
with socketserver.TCPServer(("127.0.0.1", {port}), H) as httpd:
    httpd.serve_forever()
'
"#,
            port = server_port,
            model = props_model_path,
        );
        let bin = bin_dir.path().join("neomind-llama-server");
        let mut f = std::fs::File::create(&bin).unwrap();
        f.write_all(script.as_bytes()).unwrap();
        drop(f);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        // Pre-place the model file + manifest so bootstrap's ModelMissing
        // guard passes without a network download (the download handler is
        // HTTP-triggered; bootstrap only locates).
        let store_path = data_dir.path().join("llm_backends.redb");
        let manager = Arc::new(LlmBackendInstanceManager::new(
            LlmBackendStore::open(&store_path).unwrap(),
        ));

        Self {
            data_dir,
            bin_dir,
            port: server_port,
            manager,
        }
    }

    fn cfg(&self) -> neomind_api::builtin_llm::config::BuiltinConfig {
        neomind_api::builtin_llm::config::BuiltinConfig {
            enabled: true,
            port: self.port,
            ..Default::default()
        }
    }

    fn seed_model(&self, id: &str, file: &str, sha: &str) {
        let dir = self.data_dir.path().join("models").join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(file), b"fake-gguf-bytes").unwrap();
        let manifest = serde_json::json!({
            "id": id, "version": "1.0", "file_name": file,
            "sha256": sha, "quant": "q4_k_m",
        });
        std::fs::write(
            dir.join("manifest.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .unwrap();
    }
}

/// Poll until a listener answers /health, or give up.
///
/// Fixed sleeps made the port-squat test racy: under full-workspace parallel
/// load python3 can take longer than the sleep to bind, bootstrap then won
/// the race (our fake binds successfully → ServerReady instead of the
/// expected Failed) and the suite failed intermittently — observed in
/// `cargo test --workspace`, green when the suite ran alone.
async fn wait_port_ready(port: u16, timeout: std::time::Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if neomind_api::builtin_llm::server::health_check(port).await {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    false
}

/// Env is process-global: serialize every test that touches PATH.
/// THIS LOCK SERIALIZES THE WHOLE SUITE — all three tests mutate global
/// process state (PATH, env vars, spawn children), and cargo runs test
/// binaries' tests on separate threads. Without it, A's PATH/FX changes
/// leak into B's spawn and C's assertions.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The fake llama-server is a `#!/bin/sh` wrapper around `python3 -c`, so
/// these tests need a POSIX shell + python3 (both present on CI runners and
/// any dev mac/linux box). Without them the suite would spend the 60s health
/// timeout and then fail with a confusing "server unhealthy" — skip loudly
/// instead. (Windows has no /bin/sh at all.)
fn fake_server_available() -> bool {
    if cfg!(windows) {
        return false;
    }
    std::process::Command::new("python3")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Returns false (and prints a skip notice) when the harness can't run.
macro_rules! require_fake_server {
    () => {
        if !fake_server_available() {
            eprintln!(
                "SKIP: builtin_llm_bootstrap needs /bin/sh + python3 (the fake llama-server)"
            );
            return;
        }
    };
}

fn pick_free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// The full happy chain: model on disk → bootstrap spawns the fake server
/// (discovered via PATH) → health passes → is_alive passes (real spawned
/// child) → instance registered with the REAL spawn port and the registry's
/// default ctx, and set active (no other backend was).
#[serial_test::serial]
#[tokio::test]
async fn bootstrap_full_chain_spawns_registers_activates() {
    let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    require_fake_server!();
    let port = pick_free_port();
    let model_path = "/tmp/whatever/minicpm5-2b-q4_k_m.gguf";
    let fx = Fixture::new(port, model_path);
    fx.seed_model(
        "minicpm5-2b",
        "minicpm5-2b-q4_k_m.gguf",
        "ec2d5801640099e97d8d7e8003ad4d81f336e757811f03a26173dddf386602fd",
    );

    let saved_path = std::env::var("PATH").unwrap_or_default();
    std::env::set_var(
        "PATH",
        format!("{}:{}", fx.bin_dir.path().display(), saved_path),
    );
    let outcome = bootstrap(fx.data_dir.path(), &fx.cfg(), &fx.manager).await;
    std::env::set_var("PATH", &saved_path);

    match &outcome {
        BootstrapOutcome::ServerReady { endpoint } => {
            assert_eq!(endpoint, &format!("http://127.0.0.1:{}", port));
        }
        other => panic!("expected ServerReady, got {other:?}"),
    }

    let inst = fx
        .manager
        .get_instance(BUILTIN_INSTANCE_ID)
        .expect("instance registered");
    assert_eq!(
        inst.endpoint.as_deref(),
        Some(format!("http://127.0.0.1:{}", port).as_str())
    );
    assert!(inst.is_builtin);
    assert_eq!(inst.model, "minicpm5-2b");
    assert_eq!(
        inst.capabilities.max_context, 32768,
        "registry default_ctx (32K) must land on the instance"
    );
    assert!(
        fx.manager.get_active_instance().is_some(),
        "no other backend existed — builtin must become active"
    );

    // kill_on_drop chain: dropping the registry handles must not kill the
    // child while THIS process lives — verify the server still answers.
    assert!(
        neomind_api::builtin_llm::server::health_check(port).await,
        "spawned fake server must stay alive after bootstrap returns"
    );

    // Graceful stop: the global registry must reach THIS child. Poll for the
    // death instead of sleeping (bind teardown latency varies under load).
    neomind_api::builtin_llm::server::stop_all_llama_servers();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut died = false;
    while std::time::Instant::now() < deadline {
        if !neomind_api::builtin_llm::server::health_check(port).await {
            died = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(died, "stop_all must kill the child spawned in this test");
}

/// A foreign server on our port (spawned OUTSIDE the registry) with the
/// child dying on bind must NOT be registered: the is_alive guard.
#[tokio::test]
async fn port_squatted_by_foreign_server_is_rejected() {
    let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    require_fake_server!();
    let port = pick_free_port();
    // Foreign server: binds the port BEFORE bootstrap; our fake binary would
    // also try to bind it and die. Point /props at an outside path so the
    // legacy reclaim also refuses to kill it.
    let foreign_dir = tempfile::tempdir().unwrap();
    let foreign_model = foreign_dir.path().join("foreign.gguf");
    std::fs::write(&foreign_model, b"x").unwrap();

    let fx = Fixture::new(port, foreign_model.to_str().unwrap());
    fx.seed_model(
        "minicpm5-2b",
        "minicpm5-2b-q4_k_m.gguf",
        "ec2d5801640099e97d8d7e8003ad4d81f336e757811f03a26173dddf386602fd",
    );

    // Start the foreign server manually (same fake, port pre-bound).
    // Kill-on-drop: on an assertion panic the foreign python would leak,
    // keep serving forever AND hold the test binary's inherited stdio
    // pipes open — which hangs the whole `cargo test` invocation (the
    // runner waits for EOF that never comes).
    let foreign = Fixture::new(port, foreign_model.to_str().unwrap());
    let script = foreign.bin_dir.path().join("neomind-llama-server");
    struct KillOnDrop(std::process::Child);
    impl Drop for KillOnDrop {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let _foreign_child = KillOnDrop(
        std::process::Command::new(&script)
            .spawn()
            .expect("foreign server spawn"),
    );
    assert!(
        wait_port_ready(port, std::time::Duration::from_secs(20)).await,
        "foreign server never became ready on {port}"
    );

    let saved_path = std::env::var("PATH").unwrap_or_default();
    // PUT fx's fake on PATH explicitly: this test used to rely on the PATH a
    // previously-run sibling test had left behind (env is process-global),
    // so it passed or failed depending on test order. The marker + this
    // PATH entry make the spawn deterministic: shell gate exits in ~10ms.
    std::env::set_var(
        "PATH",
        format!("{}:{}", fx.bin_dir.path().display(), saved_path),
    );
    // Deterministic bind-failure: drop the marker next to fx's fake binary;
    // the shell-level gate in the fake template exits(1) on startup.
    std::fs::write(fx.bin_dir.path().join("FAST_FAIL"), "1").unwrap();
    let outcome = bootstrap(fx.data_dir.path(), &fx.cfg(), &fx.manager).await;
    std::env::set_var("PATH", &saved_path);

    assert!(
        matches!(outcome, BootstrapOutcome::Failed(_)),
        "foreign port-squat must fail bootstrap, got {outcome:?}"
    );
    assert!(
        fx.manager.get_instance(BUILTIN_INSTANCE_ID).is_none(),
        "must not register an instance pointing at a foreign server"
    );
}

/// Stale instance record + healthy server → idempotent short-circuit, and
/// the refresh stamps effective_ctx (override honored), not the bare default.
#[serial_test::serial]
#[tokio::test]
async fn already_running_refresh_honors_ctx_override() {
    let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    require_fake_server!();
    let port = pick_free_port();
    let model_path = "/tmp/x/minicpm5-2b-q4_k_m.gguf";
    let fx = Fixture::new(port, model_path);
    fx.seed_model(
        "minicpm5-2b",
        "minicpm5-2b-q4_k_m.gguf",
        "ec2d5801640099e97d8d7e8003ad4d81f336e757811f03a26173dddf386602fd",
    );

    // Pre-register the instance (previous run) with the WRONG ctx.
    let mut inst = neomind_storage::LlmBackendInstance::new(
        BUILTIN_INSTANCE_ID.to_string(),
        "stale".to_string(),
        neomind_storage::LlmBackendType::LlamaCpp,
    );
    inst.is_builtin = true;
    inst.model = "minicpm5-2b".to_string();
    inst.capabilities.max_context = 4096;
    fx.manager.upsert_instance(inst).await.unwrap();

    // Start the fake server (outside the registry — simulates "survived
    // from the previous process").
    let mut child = std::process::Command::new(fx.bin_dir.path().join("neomind-llama-server"))
        .spawn()
        .unwrap();
    assert!(
        wait_port_ready(port, std::time::Duration::from_secs(20)).await,
        "fake server never became ready on {port}"
    );

    let mut cfg = fx.cfg();
    cfg.ctx = Some(65536); // explicit override beats the 32K registry default

    let saved_path = std::env::var("PATH").unwrap_or_default();
    std::env::set_var(
        "PATH",
        format!("{}:{}", fx.bin_dir.path().display(), saved_path),
    );
    let outcome = bootstrap(fx.data_dir.path(), &cfg, &fx.manager).await;
    std::env::set_var("PATH", &saved_path);

    assert!(
        matches!(outcome, BootstrapOutcome::ServerAlreadyRunning),
        "healthy + instance record must short-circuit, got {outcome:?}"
    );
    let inst = fx.manager.get_instance(BUILTIN_INSTANCE_ID).unwrap();
    assert_eq!(
        inst.capabilities.max_context, 65536,
        "refresh must stamp cfg.effective_ctx (override), not the bare default"
    );

    let _ = child.kill();
    let _ = child.wait();
}
