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
# Vision-chain markers (mmproj e2e): record whether the spawn carried
# --mmproj (the test asserts the flag reached argv), and switch /props
# modalities.vision on when the test planted REPORT_VISION.
case "$*" in
    *--mmproj*) printf "%s\n" "$*" > "$_script_dir/MMPROJ_ARGS" ;;
esac
if [ -f "$_script_dir/REPORT_VISION" ]; then
    export FAKE_VISION=1
fi
# "$@" is forwarded so the python payload can bind the REAL --port from the
# spawn argv — one binary then serves any port (the dual-model tests spawn
# the main server AND the vision companion through the same script).
exec python3 -c '
import http.server, socketserver, json, sys, os
port = {port}
args = sys.argv[1:]
for i in range(len(args)-1):
    if args[i] == "--port":
        port = int(args[i+1])
class H(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/health":
            body = b"ok"
        elif self.path == "/props":
            # slot_n_ctx + slot count: the instance stamps the PER-SLOT
            # share (32768 / 2 = 16384), not the raw server n_ctx.
            # modalities.vision mirrors what a projector-mounted llama-server
            # reports (off unless the test planted REPORT_VISION).
            body = json.dumps({{"model_path": "{model}", "n_ctx": 32768, "total_slots": 2, "modalities": {{"vision": os.environ.get("FAKE_VISION") == "1"}}}}).encode()
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
with socketserver.TCPServer(("127.0.0.1", port), H) as httpd:
    httpd.serve_forever()
' "$@"
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
    // The instance stamps the healthy server's /props n_ctx VERBATIM: the
    // fake reports n_ctx=32768 with total_slots=2, and that value IS the
    // server's per-request truth. (1.2.1 removed the divide-by-slots
    // budget — on an auto-slotted server it silently shrank the window 4x
    // and, since /props n_ctx already reflects partitioning, it was wrong
    // for unified-KV servers anyway.)
    assert_eq!(
        inst.capabilities.max_context, 32768,
        "server /props n_ctx must land on the instance verbatim"
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
    // Contract: the refresh stamps the OVERRIDE run through the host-RAM
    // guard — not the bare per-model default. Assert against the guard's own
    // answer: on a memory-tight host BOTH the override and the bare default
    // clamp to the 8192 floor, so a hard-coded 65536 made this test flap
    // with whatever else the machine was running.
    let def = neomind_core::builtin_llm::manifest::model_def("minicpm5-2b")
        .expect("minicpm5-2b is a registry model");
    let expected = neomind_api::builtin_llm::handlers::guarded_ctx_quiet(
        &cfg,
        port,
        def.default_ctx,
        Some(def),
    );
    assert_eq!(
        inst.capabilities.max_context, expected,
        "refresh must stamp the guarded effective ctx (override 65536, RAM-capped), \
         not the bare default; expected {expected}"
    );
    // The stale 4096 must never survive the refresh either.
    assert_ne!(inst.capabilities.max_context, 4096);

    let _ = child.kill();
    let _ = child.wait();
}

/// Vision chain: a manifest recording an mmproj (file present) makes the
/// spawn carry `--mmproj`, and the instance is stamped
/// `supports_multimodal` from the server's /props modalities — so the chat
/// picker and image-upload gate light up without waiting for a chat round.
#[serial_test::serial]
#[tokio::test]
async fn bootstrap_spawns_with_mmproj_and_stamps_vision() {
    let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    require_fake_server!();
    let port = pick_free_port();
    let model_path = "/tmp/x/gemma-4-E2B_q4_0-it.qat.gguf";
    let fx = Fixture::new(port, model_path);
    // The fake /props reports modalities.vision=true only when planted.
    std::fs::write(fx.bin_dir.path().join("REPORT_VISION"), "1").unwrap();
    fx.seed_model(
        "gemma4-e2b",
        "gemma-4-E2B_q4_0-it.qat.gguf",
        "fa401b55b07ee70a54c6dae3903c783a6e65064312529ea57175cb5f8dec6634",
    );
    // Projector file + a manifest that records it (seed_model's manifest
    // has no mmproj — overwrite with the vision shape).
    let mdir = fx.data_dir.path().join("models").join("gemma4-e2b");
    std::fs::write(
        mdir.join("gemma-4-E2B-it-mmproj.gguf"),
        b"fake-mmproj-bytes",
    )
    .unwrap();
    let manifest = serde_json::json!({
        "id": "gemma4-e2b", "version": "1.0",
        "file_name": "gemma-4-E2B_q4_0-it.qat.gguf",
        "sha256": "fa401b55b07ee70a54c6dae3903c783a6e65064312529ea57175cb5f8dec6634",
        "quant": "qat_q4_0",
        "mmproj": {"file_name": "gemma-4-E2B-it-mmproj.gguf",
                    "sha256": "021059cce659fe7f9170d5599761d7bbaf644b798dab9503aca30dc43e6beb14"}
    });
    std::fs::write(
        mdir.join("manifest.json"),
        serde_json::to_string(&manifest).unwrap(),
    )
    .unwrap();

    let saved_path = std::env::var("PATH").unwrap_or_default();
    std::env::set_var(
        "PATH",
        format!("{}:{}", fx.bin_dir.path().display(), saved_path),
    );
    let outcome = bootstrap(fx.data_dir.path(), &fx.cfg(), &fx.manager).await;
    std::env::set_var("PATH", &saved_path);

    assert!(
        matches!(outcome, BootstrapOutcome::ServerReady { .. }),
        "expected ServerReady, got {outcome:?}"
    );

    // argv check: the projector flag AND file reached llama-server.
    let marker = fx.bin_dir.path().join("MMPROJ_ARGS");
    assert!(
        marker.exists(),
        "spawn must carry --mmproj when the manifest records a present projector"
    );
    let args = std::fs::read_to_string(&marker).unwrap();
    assert!(args.contains("--mmproj"), "args: {args}");
    assert!(
        args.contains("gemma-4-E2B-it-mmproj.gguf"),
        "projector path must be the argv value, args: {args}"
    );

    let inst = fx
        .manager
        .get_instance(BUILTIN_INSTANCE_ID)
        .expect("instance registered");
    assert!(
        inst.capabilities.supports_multimodal,
        "/props modalities.vision must stamp the instance at spawn time"
    );

    neomind_api::builtin_llm::server::stop_all_llama_servers();
}

/// Manifest records a projector but the FILE is gone (partial delete /
/// failed mmproj download) → the spawn degrades to text-only: ServerReady,
/// no --mmproj on argv, and no vision capability stamped (the fake reports
/// modalities.vision=true — /props is the authority, attachment is not).
#[serial_test::serial]
#[tokio::test]
async fn missing_mmproj_file_degrades_to_text_only() {
    let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    require_fake_server!();
    let port = pick_free_port();
    let model_path = "/tmp/x/gemma-4-E2B_q4_0-it.qat.gguf";
    let fx = Fixture::new(port, model_path);
    // /props claims vision — the stamp must still be false because the
    // projector never reached argv... the server LIES here on purpose:
    // query_server_modalities_vision would stamp true. The degrade contract
    // is therefore "no --mmproj flag", not "capability false" — a lying
    // /props is exactly what the spawn-time probe trusts by design.
    std::fs::write(fx.bin_dir.path().join("REPORT_VISION"), "1").unwrap();
    fx.seed_model(
        "gemma4-e2b",
        "gemma-4-E2B_q4_0-it.qat.gguf",
        "fa401b55b07ee70a54c6dae3903c783a6e65064312529ea57175cb5f8dec6634",
    );
    // Manifest records a projector whose file does NOT exist.
    let mdir = fx.data_dir.path().join("models").join("gemma4-e2b");
    let manifest = serde_json::json!({
        "id": "gemma4-e2b", "version": "1.0",
        "file_name": "gemma-4-E2B_q4_0-it.qat.gguf",
        "sha256": "fa401b55b07ee70a54c6dae3903c783a6e65064312529ea57175cb5f8dec6634",
        "quant": "qat_q4_0",
        "mmproj": {"file_name": "gemma-4-E2B-it-mmproj.gguf", "sha256": "021059cc"}
    });
    std::fs::write(
        mdir.join("manifest.json"),
        serde_json::to_string(&manifest).unwrap(),
    )
    .unwrap();

    let saved_path = std::env::var("PATH").unwrap_or_default();
    std::env::set_var(
        "PATH",
        format!("{}:{}", fx.bin_dir.path().display(), saved_path),
    );
    let outcome = bootstrap(fx.data_dir.path(), &fx.cfg(), &fx.manager).await;
    std::env::set_var("PATH", &saved_path);

    assert!(
        matches!(outcome, BootstrapOutcome::ServerReady { .. }),
        "missing projector must degrade, not fail: {outcome:?}"
    );
    assert!(
        !fx.bin_dir.path().join("MMPROJ_ARGS").exists(),
        "text-only degrade must not pass --mmproj"
    );

    neomind_api::builtin_llm::server::stop_all_llama_servers();
}

/// Seed the registry's vision-companion model (minicpm-v-4.7-1b, role=vlm)
/// with its projector on disk — the dual-model tests' companion payload.
fn seed_companion(fx: &Fixture) {
    fx.seed_model(
        "minicpm-v-4.7-1b",
        "minicpm-v-4.7-1b-q4_k_m.gguf",
        "588c6c94c82f756b86dba35c42ed9ba9ec187d65824eb52f039c347df563e63d",
    );
    let cdir = fx.data_dir.path().join("models").join("minicpm-v-4.7-1b");
    std::fs::write(
        cdir.join("minicpm-v-4.7-1b-mmproj-f16.gguf"),
        b"fake-mmproj-bytes",
    )
    .unwrap();
    // seed_model's manifest has no mmproj — rewrite with the vision shape.
    let manifest = serde_json::json!({
        "id": "minicpm-v-4.7-1b", "version": "1.0",
        "file_name": "minicpm-v-4.7-1b-q4_k_m.gguf",
        "sha256": "588c6c94c82f756b86dba35c42ed9ba9ec187d65824eb52f039c347df563e63d",
        "quant": "q4_k_m",
        "mmproj": {"file_name": "minicpm-v-4.7-1b-mmproj-f16.gguf",
                    "sha256": "fb9cc047a3cdad5317fd8433a4aeda2d22f00f02ca4bd93b34ad84165b25a900"}
    });
    std::fs::write(
        cdir.join("manifest.json"),
        serde_json::to_string(&manifest).unwrap(),
    )
    .unwrap();
}

/// Dual-model bootstrap: main text model AND the vision companion come up on
/// their own ports; the companion instance is registered with vision
/// capabilities but is NEVER the active chat backend.
#[serial_test::serial]
#[tokio::test]
async fn bootstrap_spawns_vision_companion_alongside_main() {
    let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    require_fake_server!();
    let main_port = pick_free_port();
    let vlm_port = pick_free_port();
    let fx = Fixture::new(main_port, "/tmp/x/minicpm5-2b-q4_k_m.gguf");
    // /props reports vision for both spawns (same script + env) — fine: the
    // assertions below only read the COMPANION's capability stamp.
    std::fs::write(fx.bin_dir.path().join("REPORT_VISION"), "1").unwrap();
    fx.seed_model(
        "minicpm5-2b",
        "minicpm5-2b-q4_k_m.gguf",
        "ec2d5801640099e97d8d7e8003ad4d81f336e757811f03a26173dddf386602fd",
    );
    seed_companion(&fx);

    let mut cfg = fx.cfg();
    cfg.vlm_port = vlm_port;

    let saved_path = std::env::var("PATH").unwrap_or_default();
    std::env::set_var(
        "PATH",
        format!("{}:{}", fx.bin_dir.path().display(), saved_path),
    );
    let outcome = bootstrap(fx.data_dir.path(), &cfg, &fx.manager).await;
    std::env::set_var("PATH", &saved_path);

    assert!(
        matches!(outcome, BootstrapOutcome::ServerReady { .. }),
        "expected ServerReady, got {outcome:?}"
    );

    // Main first: active as usual.
    let active_id = fx.manager.get_active_instance().map(|i| i.id);
    assert_eq!(active_id.as_deref(), Some("builtin-lfm25-2.6b"));

    // Companion: registered, vision-capable, describe-only, NEVER active.
    let comp = fx
        .manager
        .get_instance("builtin-vlm-minicpm-v-4.7-1b")
        .expect("companion instance registered");
    assert!(
        !comp.is_active,
        "companion must never be the active backend"
    );
    assert!(comp.is_builtin);
    assert!(
        comp.capabilities.supports_multimodal,
        "vision capability stamped"
    );
    assert!(
        !comp.capabilities.supports_tools,
        "companion is describe-only"
    );
    assert_eq!(
        comp.endpoint.as_deref(),
        Some(format!("http://127.0.0.1:{}", vlm_port).as_str())
    );

    // argv proof: the projector AND the companion model reached llama-server
    // on the companion port (marker content carries the full argv).
    let marker = fx.bin_dir.path().join("MMPROJ_ARGS");
    assert!(marker.exists(), "companion spawn must carry --mmproj");
    let args = std::fs::read_to_string(&marker).unwrap();
    assert!(args.contains("--mmproj"), "args: {args}");
    assert!(
        args.contains("minicpm-v-4.7-1b"),
        "companion model in argv: {args}"
    );
    assert!(
        args.contains(&format!("--port {vlm_port}")),
        "companion on its own port: {args}"
    );

    neomind_api::builtin_llm::server::stop_all_llama_servers();
}

/// NEOMIND_BUILTIN_VLM=off (cfg.vlm_enabled=false): the companion is skipped
/// entirely — no instance, no second server — and the MAIN model's outcome
/// is untouched.
#[serial_test::serial]
#[tokio::test]
async fn vlm_disabled_skips_companion_main_unaffected() {
    let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    require_fake_server!();
    let main_port = pick_free_port();
    let vlm_port = pick_free_port();
    let fx = Fixture::new(main_port, "/tmp/x/minicpm5-2b-q4_k_m.gguf");
    fx.seed_model(
        "minicpm5-2b",
        "minicpm5-2b-q4_k_m.gguf",
        "ec2d5801640099e97d8d7e8003ad4d81f336e757811f03a26173dddf386602fd",
    );
    seed_companion(&fx);

    let mut cfg = fx.cfg();
    cfg.vlm_port = vlm_port;
    cfg.vlm_enabled = false;

    let saved_path = std::env::var("PATH").unwrap_or_default();
    std::env::set_var(
        "PATH",
        format!("{}:{}", fx.bin_dir.path().display(), saved_path),
    );
    let outcome = bootstrap(fx.data_dir.path(), &cfg, &fx.manager).await;
    std::env::set_var("PATH", &saved_path);

    assert!(
        matches!(outcome, BootstrapOutcome::ServerReady { .. }),
        "main model must be unaffected: {outcome:?}"
    );
    assert!(
        fx.manager
            .list_instances()
            .iter()
            .all(|i| i.id != "builtin-vlm-minicpm-v-4.7-1b"),
        "disabled companion must not register an instance"
    );
    assert!(
        !neomind_api::builtin_llm::server::health_check(vlm_port).await,
        "no second server may run with the feature off"
    );

    neomind_api::builtin_llm::server::stop_all_llama_servers();
}
