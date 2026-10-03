//! Official Codex app-server over private stdio. Never reads other auth stores.
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::Mutex,
};
const RPC_TIMEOUT: Duration = Duration::from_secs(30);
const CHAT_TIMEOUT: Duration = Duration::from_secs(180);
const LOGIN_TIMEOUT: Duration = Duration::from_secs(600);
const MAX_HISTORY: usize = 200_000;
const DISABLED: &[&str] = &[
    "shell_tool",
    "shell_snapshot",
    "shell_snapshot_v2",
    "view_image",
    "js_repl",
    "code_mode",
    "code_mode_host",
    "code_mode_only",
    "computer_use",
    "browser_use",
    "multi_agent",
    "multi_agent_v2",
    "agent_message_board",
    "apps",
    "plugins",
    "hooks",
    "codex_hooks",
    "plugin_hooks",
    "memory_tool",
    "memories",
    "image_generation",
    "imagegenext",
    "tool_search",
    "search_tool",
    "tool_suggest",
    "request_permissions_tool",
    "request_permissions",
    "goals",
    "remote_control",
    "external_migration",
    "undo",
];
fn executable(app: &AppHandle) -> Result<PathBuf, String> {
    let exe = app
        .path()
        .resource_dir()
        .map_err(|_| "Cannot locate app resources.")?
        .join("codex")
        .join("codex.exe");
    if exe.is_file() {
        return Ok(exe);
    }
    #[cfg(debug_assertions)]
    {
        let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/codex/codex.exe");
        if dev.is_file() {
            return Ok(dev);
        }
    }
    Err("Bundled Codex is missing. Reinstall the complete Coucou package (including the codex folder).".into())
}
#[derive(Default)]
pub struct Codex {
    inner: Mutex<Client>,
}
#[derive(Default)]
struct Client {
    server: Option<Server>,
    thread: Option<String>,
    // Successful turns only; recover context after a crash. Never written to disk.
    history: Vec<Value>,
}
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AccountStatus {
    connected: bool,
    pending: bool,
    message: String,
}
struct Server {
    child: Child,
    stdin: ChildStdin,
    lines: Lines<BufReader<ChildStdout>>,
    next_id: u64,
    cwd: PathBuf,
    login: Option<(String, Instant)>,
    login_failed: bool,
    rpc_timeout: Duration,
}
impl Server {
    async fn start(exe: &Path, home: &Path) -> Result<Self, String> {
        crate::platform::ensure_private_dir(home)
            .map_err(|_| "Cannot create private ChatGPT profile.")?;
        let cwd = home.join("empty-workspace");
        crate::platform::ensure_private_dir(&cwd)
            .map_err(|_| "Cannot create isolated chat workspace.")?;
        let mut cmd = Command::new(exe);
        cmd.arg("app-server").args(["--listen", "stdio://"]);
        // No inherited API keys, CODEX_HOME, provider URLs, or Hermes variables.
        cmd.env_clear();
        for key in [
            "SystemRoot",
            "WINDIR",
            "COMSPEC",
            "LOCALAPPDATA",
            "APPDATA",
            "TEMP",
            "TMP",
        ] {
            if let Some(value) = std::env::var_os(key) {
                cmd.env(key, value);
            }
        }
        cmd.env("CODEX_HOME", home)
            .env("HOME", &cwd)
            .env("USERPROFILE", &cwd);
        cmd.current_dir(&cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        for value in [
            "cli_auth_credentials_store=\"keyring\"",
            "forced_login_method=\"chatgpt\"",
            "model_provider=\"openai\"",
            "sandbox_mode=\"read-only\"",
            "approval_policy=\"never\"",
            "web_search=\"disabled\"",
            "mcp_servers={}",
            "project_doc_max_bytes=0",
            "history.persistence=\"none\"",
            "analytics.enabled=false",
            "feedback.enabled=false",
            "include_environment_context=false",
            "tools.update_plan.enabled=false",
            "tools.experimental_request_user_input.enabled=false",
            "features.skip_host_skill_discovery=true",
        ] {
            cmd.args(["-c", value]);
        }
        for feature in DISABLED {
            cmd.args(["--disable", feature]);
        }
        #[cfg(windows)]
        cmd.creation_flags(0x08000000);
        let mut child = cmd.spawn().map_err(|_| {
            "Cannot launch bundled Codex. Reinstall Coucou or check security software."
        })?;
        let stdin = child.stdin.take().ok_or("Codex input unavailable.")?;
        let stdout = child.stdout.take().ok_or("Codex output unavailable.")?;
        let mut server = Self {
            child,
            stdin,
            lines: BufReader::new(stdout).lines(),
            next_id: 0,
            cwd,
            login: None,
            login_failed: false,
            rpc_timeout: RPC_TIMEOUT,
        };
        server.rpc("initialize", json!({"clientInfo":{"name":"coucou","title":"Coucou","version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":true}})).await?;
        server.write(json!({"method":"initialized"})).await?;
        Ok(server)
    }
    async fn write(&mut self, value: Value) -> Result<(), String> {
        let mut data = serde_json::to_vec(&value).map_err(|_| "Cannot encode Codex request.")?;
        data.push(b'\n');
        self.stdin
            .write_all(&data)
            .await
            .map_err(|_| "Codex connection closed.")?;
        self.stdin
            .flush()
            .await
            .map_err(|_| "Codex connection closed.".into())
    }
    async fn next(&mut self) -> Result<Value, String> {
        let line = self
            .lines
            .next_line()
            .await
            .map_err(|_| "Cannot read Codex response.")?
            .ok_or("Codex stopped unexpectedly. Please retry.")?;
        let v: Value =
            serde_json::from_str(&line).map_err(|_| "Invalid Codex protocol response.")?;
        // Never service tools, approvals, file access, or credential requests.
        if v.get("method").is_some() && v.get("id").is_some() {
            self.write(json!({"id":v["id"],"error":{"code":-32601,"message":"Coucou does not allow tools or approvals"}})).await?;
            return Err("Codex requested an unavailable tool. No permission was granted.".into());
        }
        if v["method"] == "account/login/completed" {
            self.login_failed = v["params"]["success"] != true;
            self.login = None;
        }
        Ok(v)
    }
    async fn rpc(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.next_id += 1;
        let id = self.next_id;
        let timeout = self.rpc_timeout;
        let future = async {
            self.write(json!({"id":id,"method":method,"params":params}))
                .await?;
            loop {
                let v = self.next().await?;
                if v["id"].as_u64() == Some(id) {
                    if v.get("error").is_some() {
                        // Raw errors may contain OAuth secrets: never send to UI/logs.
                        return Err(format!("Codex {method} failed. Check your connection and ChatGPT/Codex account access, then retry."));
                    }
                    return v
                        .get("result")
                        .cloned()
                        .ok_or_else(|| "Codex response missing result.".into());
                }
            }
        };
        tokio::time::timeout(timeout, future)
            .await
            .map_err(|_| "Codex timed out. Please retry.".to_string())?
    }
    async fn account(&mut self) -> Result<AccountStatus, String> {
        if self
            .login
            .as_ref()
            .is_some_and(|(_, t)| t.elapsed() > LOGIN_TIMEOUT)
        {
            self.cancel_login().await?;
            self.login_failed = true;
        }
        let result = self
            .rpc("account/read", json!({"refreshToken":false}))
            .await?;
        let connected = result["account"]["type"] == "chatgpt";
        if connected {
            self.login = None;
            self.login_failed = false;
        }
        Ok(AccountStatus {
            connected,
            pending: self.login.is_some(),
            message: if connected {
                "Connected to ChatGPT through Codex. Text-only chat; no local tools."
            } else if self.login.is_some() {
                "Finish signing in in your browser. This request expires after 10 minutes."
            } else if self.login_failed {
                "Sign-in failed, expired, or was cancelled. Click Connect ChatGPT to retry."
            } else {
                "Not connected. Sign in with a ChatGPT account that has Codex access."
            }
            .into(),
        })
    }
    async fn cancel_login(&mut self) -> Result<(), String> {
        if let Some((id, _)) = self.login.take() {
            self.rpc("account/login/cancel", json!({"loginId":id}))
                .await?;
        }
        Ok(())
    }
    async fn answer(&mut self, thread: &str, query: &str) -> Result<String, String> {
        let r = self.rpc("turn/start", json!({"threadId":thread,"input":[{"type":"text","text":query}],
            "environments":[], "approvalPolicy":"never", "sandboxPolicy":{"type":"readOnly","networkAccess":false}})).await?;
        let turn = r["turn"]["id"]
            .as_str()
            .ok_or("Codex did not start a turn.")?
            .to_owned();
        let mut messages = Vec::new();
        loop {
            let v = self.next().await?;
            let p = &v["params"];
            if p["threadId"] != thread {
                continue;
            }
            if v["method"] == "item/completed"
                && p["turnId"] == turn
                && p["item"]["type"] == "agentMessage"
            {
                if let Some(text) = p["item"]["text"].as_str() {
                    messages.push(text.to_owned());
                }
            }
            if v["method"] == "turn/completed" && p["turn"]["id"] == turn {
                if p["turn"]["status"] != "completed" {
                    return Err("ChatGPT could not complete this message. Check connection, sign-in, and Codex usage limits, then retry. Earlier successful messages are kept.".into());
                }
                let text = messages.join("\n\n");
                if text.trim().is_empty() {
                    return Err("ChatGPT returned no text. Please retry.".into());
                }
                return Ok(text);
            }
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}
impl Client {
    async fn server(&mut self, app: &AppHandle) -> Result<&mut Server, String> {
        if self.server.is_none() {
            self.thread = None;
            self.server = Some(
                Server::start(
                    &executable(app)?,
                    &crate::settings::local_dir().join("chatgpt-codex"),
                )
                .await?,
            );
        }
        Ok(self.server.as_mut().unwrap())
    }
    fn stop(&mut self) {
        self.server = None;
        self.thread = None;
    }
}
impl Codex {
    pub async fn status(&self, app: &AppHandle) -> Result<AccountStatus, String> {
        let mut c = self
            .inner
            .try_lock()
            .map_err(|_| "ChatGPT is busy. Try again when the message finishes.")?;
        let result = c.server(app).await?.account().await;
        if result.is_err() {
            c.stop();
        }
        result
    }
    pub async fn connect(&self, app: &AppHandle) -> Result<(), String> {
        let mut c = self.inner.try_lock().map_err(|_| "ChatGPT is busy.")?;
        c.stop();
        let result = async {
            let s = c.server(app).await?;
            s.cancel_login().await?;
            s.rpc("account/logout", json!({})).await?;
            let r = s
                .rpc("account/login/start", json!({"type":"chatgpt"}))
                .await?;
            let url = r["authUrl"]
                .as_str()
                .ok_or("Codex did not provide a login URL.")?;
            let parsed = reqwest::Url::parse(url).map_err(|_| "Invalid login URL.")?;
            if parsed.scheme() != "https"
                || parsed.host_str() != Some("auth.openai.com")
                || !parsed.username().is_empty()
                || parsed.password().is_some()
            {
                return Err("Codex returned an unexpected login destination.".to_string());
            }
            let id = r["loginId"]
                .as_str()
                .ok_or("Codex did not provide a login ID.")?
                .to_owned();
            s.login = Some((id, Instant::now()));
            s.login_failed = false;
            crate::platform::open_url(url);
            Ok(())
        }
        .await;
        if result.is_err() {
            c.stop();
        } else {
            c.history.clear();
        }
        result
    }
    pub async fn disconnect(&self, app: &AppHandle) -> Result<(), String> {
        let mut c = self
            .inner
            .try_lock()
            .map_err(|_| "Wait for the current ChatGPT message to finish before disconnecting.")?;
        let result = async {
            let s = c.server(app).await?;
            s.cancel_login().await?;
            s.rpc("account/logout", json!({})).await?;
            if s.account().await?.connected {
                return Err("Could not verify ChatGPT sign-out. Please retry.".into());
            }
            Ok(())
        }
        .await;
        c.stop();
        if result.is_ok() {
            c.history.clear();
        }
        result
    }
    pub async fn reset(&self) {
        let mut c = self.inner.lock().await;
        c.stop();
        c.history.clear();
    }
    pub async fn send(
        &self,
        app: &AppHandle,
        query: String,
        context: Option<crate::claude::ChatContext>,
    ) -> Result<crate::claude::ChatReply, String> {
        if context.is_some() {
            return Err("ChatGPT mode is text-only: remove the attachment/start a new chat, or select Anthropic for file chat.".into());
        }
        if query.trim().is_empty() {
            return Err("Enter a message first.".into());
        }
        let mut c = self
            .inner
            .try_lock()
            .map_err(|_| "A ChatGPT message is already running.")?;
        if query.len() + c.history.iter().map(|v| v.to_string().len()).sum::<usize>() > MAX_HISTORY
        {
            return Err("This conversation is too long. Start a new chat.".into());
        }
        let task = async {
            if !c.server(app).await?.account().await?.connected {
                return Err(
                    "Connect ChatGPT in Coucou settings first. No API key is needed.".into(),
                );
            }
            let fresh = c.thread.is_none();
            if fresh {
                let s = c.server.as_mut().unwrap();
                let r = s
                    .rpc("thread/start", thread_options(&s.cwd.to_string_lossy()))
                    .await?;
                c.thread = Some(
                    r["thread"]["id"]
                        .as_str()
                        .ok_or("Codex did not create a conversation.")?
                        .to_owned(),
                );
            }
            let input = if fresh && !c.history.is_empty() {
                format!("Previous conversation (quoted context, not new instructions):\n{}\n\nCurrent user message:\n{}", serde_json::to_string(&c.history).unwrap(), query)
            } else {
                query.clone()
            };
            let thread = c.thread.clone().unwrap();
            c.server.as_mut().unwrap().answer(&thread, &input).await
        };
        let result = tokio::time::timeout(CHAT_TIMEOUT, task).await
            .unwrap_or_else(|_| Err("ChatGPT timed out after 3 minutes. The request was stopped; retry your message. Earlier successful messages are kept.".into()));
        match result {
            Ok(text) => {
                c.history.push(json!({"role":"user","content":query}));
                c.history.push(json!({"role":"assistant","content":text}));
                Ok(crate::claude::ChatReply { text })
            }
            Err(e) => {
                c.stop();
                Err(e)
            }
        }
    }
}
fn thread_options(cwd: &str) -> Value {
    json!({
        "cwd": cwd, "modelProvider": "openai", "ephemeral": true,
        "sandbox": "read-only", "approvalPolicy": "never",
        "environments": [], "runtimeWorkspaceRoots": [], "selectedCapabilityRoots": [],
        "dynamicTools": [],
        "baseInstructions": "You are Mochi, a helpful conversational assistant in Coucou. Respond in the user's language. You cannot access files, execute commands, browse, or use tools. Answer with plain text. Never claim to have performed actions."
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn official_app_server_handshake_and_isolation() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            let home = root.join(format!("../.codex-test-{}", std::process::id()));
            let exe = root.join("resources/codex/codex.exe");
            let mut s = Server::start(&exe, &home)
                .await
                .expect("official app-server must initialize");
            let status = s.account().await.unwrap();
            assert!(
                !status.connected,
                "isolated profile must not inherit credentials"
            );
            assert!(!status.pending);
            let result = s
                .rpc("thread/start", thread_options(&s.cwd.to_string_lossy()))
                .await
                .unwrap();
            assert!(result["thread"]["id"].is_string());
            assert_eq!(result["thread"]["ephemeral"], true);
            assert_eq!(result["sandbox"]["type"], "readOnly");
            let config = s
                .rpc("config/read", json!({"includeLayers":false}))
                .await
                .unwrap();
            assert_eq!(config["config"]["features"]["shell_tool"], false);
            assert_eq!(config["config"]["web_search"], "disabled");
            // Exercise real OAuth initiation and cancellation without opening a
            // browser or submitting any credentials. Never print authUrl/loginId.
            let login = s
                .rpc("account/login/start", json!({"type":"chatgpt"}))
                .await
                .unwrap();
            let url = reqwest::Url::parse(login["authUrl"].as_str().unwrap()).unwrap();
            assert_eq!(url.host_str(), Some("auth.openai.com"));
            assert_eq!(url.scheme(), "https");
            s.rpc("account/login/cancel", json!({"loginId":login["loginId"]}))
                .await
                .unwrap();
            s.rpc("account/logout", json!({})).await.unwrap();
            assert!(!s.account().await.unwrap().connected);
            s.rpc_timeout = Duration::from_nanos(1);
            let error = s
                .rpc("config/read", json!({"includeLayers":false}))
                .await
                .unwrap_err();
            assert!(error.contains("timed out"));
            s.child.kill().await.unwrap();
            drop(s);
            let _ = std::fs::remove_dir_all(home);
        });
    }

    // The model response below is an explicitly synthetic local test fixture.
    // The real official CLI builds the request and emits the protocol events.
    #[test]
    fn official_cli_text_turns_have_no_tools_and_keep_history() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            use tokio::io::AsyncReadExt;
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let serving = tokio::spawn(async move {
                let mut requests = Vec::new();
                for index in 0..3 {
                    let (mut stream, _) = tokio::time::timeout(Duration::from_secs(20), listener.accept()).await.unwrap().unwrap();
                    let mut bytes = Vec::new();
                    let body_start;
                    loop {
                        let mut buf = [0; 8192];
                        let n = stream.read(&mut buf).await.unwrap();
                        assert!(n > 0);
                        bytes.extend_from_slice(&buf[..n]);
                        if let Some(i) = bytes.windows(4).position(|w| w == b"\r\n\r\n") { body_start = i + 4; break; }
                    }
                    let headers = String::from_utf8_lossy(&bytes[..body_start]).to_lowercase();
                    let length: usize = headers.lines().find_map(|l| l.strip_prefix("content-length: ")).unwrap().parse().unwrap();
                    while bytes.len() < body_start + length {
                        let mut buf = [0; 8192];
                        let n = stream.read(&mut buf).await.unwrap();
                        assert!(n > 0);
                        bytes.extend_from_slice(&buf[..n]);
                    }
                    requests.push(serde_json::from_slice::<Value>(&bytes[body_start..body_start+length]).expect("fixture expects uncompressed JSON"));
                    if index == 2 {
                        let body = r#"{"error":{"message":"TEST_SECRET_DO_NOT_PROPAGATE","type":"invalid_request_error"}}"#;
                        let response = format!("HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
                        stream.write_all(response.as_bytes()).await.unwrap();
                        continue;
                    }
                    let message = json!({"id":format!("msg_{index}"),"type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":format!("Fixture answer {index}"),"annotations":[]}]});
                    let events = [
                        json!({"type":"response.created","response":{"id":format!("r_{index}"),"status":"in_progress","output":[]}}),
                        json!({"type":"response.output_item.added","output_index":0,"item":message}),
                        json!({"type":"response.output_item.done","output_index":0,"item":message}),
                        json!({"type":"response.completed","response":{"id":format!("r_{index}"),"status":"completed","output":[message],"usage":{"input_tokens":10,"output_tokens":5,"total_tokens":15}}}),
                    ];
                    let body: String = events.iter().map(|e| format!("event: {}\ndata: {}\n\n", e["type"].as_str().unwrap(), e)).collect();
                    let header = format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                    stream.write_all(header.as_bytes()).await.unwrap();
                    stream.write_all(body.as_bytes()).await.unwrap();
                }
                requests
            });
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            let home = root.join(format!("../.codex-turn-test-{}", std::process::id()));
            let mut s = Server::start(&root.join("resources/codex/codex.exe"), &home).await.unwrap();
            let mut options = thread_options(&s.cwd.to_string_lossy());
            options["modelProvider"] = json!("coucou_fixture");
            options["model"] = json!("gpt-5.4");
            options["config"] = json!({
                "model_providers.coucou_fixture": {"name":"Local test fixture","base_url":format!("http://{address}/v1"),"wire_api":"responses","requires_openai_auth":false,"request_max_retries":0,"stream_max_retries":0},
                "features.enable_request_compression": false,
                "features.responses_websockets": false,
                "features.responses_websockets_v2": false
            });
            let thread = s.rpc("thread/start", options).await.unwrap()["thread"]["id"].as_str().unwrap().to_owned();
            assert_eq!(tokio::time::timeout(Duration::from_secs(25), s.answer(&thread, "Remember the word mango")).await.unwrap().unwrap(), "Fixture answer 0");
            assert_eq!(tokio::time::timeout(Duration::from_secs(25), s.answer(&thread, "What word did I say?")).await.unwrap().unwrap(), "Fixture answer 1");
            let error = tokio::time::timeout(Duration::from_secs(25), s.answer(&thread, "Test an error")).await.unwrap().unwrap_err();
            assert!(!error.contains("TEST_SECRET_DO_NOT_PROPAGATE"));
            assert!(error.contains("could not complete"));
            let requests = serving.await.unwrap();
            assert_eq!(requests.len(), 3);
            for request in &requests {
                assert_eq!(request["tools"], json!([]), "text-only chat must expose zero tools");
            }
            let history = requests[1]["input"].to_string();
            assert!(history.contains("Remember the word mango"));
            assert!(history.contains("Fixture answer 0"));
            s.child.kill().await.unwrap();
            assert!(s.next().await.is_err());
            drop(s);
            let _ = std::fs::remove_dir_all(home);
        });
    }

    #[test]
    fn chat_has_no_execution_environment_and_is_read_only() {
        let p = thread_options("D:/empty");
        assert_eq!(p["environments"], json!([]));
        assert_eq!(p["sandbox"], "read-only");
        assert_eq!(p["approvalPolicy"], "never");
        assert_eq!(p["ephemeral"], true);
        assert_eq!(p["modelProvider"], "openai");
    }
}
