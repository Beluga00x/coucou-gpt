# ChatGPT account chat (Windows x64)

Coucou now has **Settings → Chat provider → ChatGPT account (Codex)**. Select it,
click **Connect ChatGPT**, complete the official OpenAI page in your browser, and
wait for Connected (or click Check status). No API key is needed. The account
must have Codex access and remaining usage allowance. This is the Codex service
using ChatGPT account authentication, not a connection to ChatGPT web chat history.
Anthropic and its separate API-key/model settings remain available.

## Implementation and safety

- Official `@openai/codex` **0.160.0**, pinned in package.json/package-lock.json.
  Rust launches the native bundled `codex/codex.exe app-server --listen stdio://`.
  Node, npm, a global CLI, and PATH lookup are not needed in the installed app.
- OAuth: `initialize`, `account/login/start` with `type: chatgpt`,
  `account/read`, `account/login/cancel`, `account/logout`. Only Rust receives
  the authorization URL and it permits HTTPS `auth.openai.com` before opening
  the system browser. The webview receives only status booleans and safe messages.
- Isolated `CODEX_HOME`: `%LOCALAPPDATA%\Coucou\chatgpt-codex`.
  `cli_auth_credentials_store=keyring` requires Windows Credential Manager;
  there is no plaintext fallback. Codex owns credential storage and refresh.
  Coucou does not read auth.json, browser cookies, another Codex profile, or
  Hermes credentials. The child receives a small allowlist of Windows runtime
  environment variables, not inherited API keys/provider URLs/proxy secrets.
- Text-only: threads AND turns explicitly select `environments: []`; no local
  execution environment or runtime workspace roots are exposed. This is important:
  merely disabling the old apply_patch feature is NOT sufficient for this CLI.
  Shell, JS/code mode, browser/computer use, MCP apps, plugins, hooks, memory,
  image tools, delegation and permission tools are disabled. Web search disabled;
  no dynamic tools supplied. Read-only sandbox and `approvalPolicy: never` are
  additional restrictions. Child cwd is a separate empty-workspace directory.
  Unexpected server-side tool/approval requests are rejected, never executed.
- The real CLI's outbound model request was captured against a local test fixture:
  `tools` was an empty array on all tested turns. No repo/user file content is
  passed. Attachments/window context are explicitly rejected in ChatGPT mode.
- Native ephemeral threads keep conversation context. Only successful turns are
  retained in Coucou memory; a failed request is removed from the UI and restored
  to the input box. After a server error, successful text history is supplied as
  quoted context to a fresh thread. New chat (`+`), switching providers,
  connecting another account, disconnecting, and app restart clear conversation.
- RPC deadline: 30 seconds; whole chat: 180 seconds; login: 10 minutes (checked
  when status is polled). Child is killed on drop/error/timeout. Raw server errors,
  stderr, OAuth URLs, account details and tokens are not logged or forwarded.
- Telemetry/feedback explicitly disabled. This integration uses experimental
  app-server fields. Review/retest security controls before changing the CLI pin.

## Build and deployment

From `D:/coucou/windows` with Rust/MSVC and Node installed:

```sh
export PATH=/c/Users/osand/.cargo/bin:$PATH
npm ci
node scripts/prepare-codex.mjs
cargo test -p coucou --lib
npm run test:chatgpt-ui
npm run pack
```

The UI smoke test starts/stops its own Vite on 127.0.0.1:1421 and uses installed
Microsoft Edge headless. Its Tauri IPC is explicitly mocked; it does not sign in.
The Rust tests separately launch the real official native CLI in temporary,
isolated profiles. `prepare-codex` also runs automatically during prebuild/predev.

Deploy using `release/Coucou-Windows-setup.exe`. If deploying manually, copy BOTH
`target/release/coucou.exe` and the complete `target/release/codex/` folder beside
it (retain the existing coucou-hook.exe). The native CLI, code-mode helper (not
used/enabled), README, Apache license and NOTICE are bundled by Tauri.
`src-tauri/resources/codex/` is generated, gitignored, and recreated from npm.
No running installed executable was replaced as part of implementation.

## Verification and remaining user step

Verified automatically:
- 12 Rust library tests pass, including legacy settings migration, actual CLI
  handshake, isolated signed-out status, read-only thread creation, official
  browser-OAuth URL generation/cancellation, logout read-back, RPC timeout,
  two local-fixture replies with full conversation history and **zero tools**,
  redaction of a synthetic 401 server error, and closed-process handling.
- Edge component smoke tests pass for provider selection, Connect IPC, status,
  disconnect, reply rendering, New chat reset, and failed-turn retry behavior.
- TypeScript/Vite and Windows release/NSIS packaging succeed.

**Not verified:** successful account authentication and a live OpenAI model reply.
No user login was performed and no credentials were requested. The user must
complete the browser login after deployment, then send a message and a follow-up.
Browser login requires Codex's loopback callback port to be available; a conflict,
network restrictions, missing account access, or quota limits can prevent it.
Errors are intentionally generic to avoid disclosing credential-bearing details.

Manual acceptance: select ChatGPT → Connect → complete browser consent → Connected
→ send “Reply in Sinhala” → ask a contextual follow-up → New chat → Disconnect
→ check signed-out status. Also verify Anthropic still works if a key is available.

## Official references consulted

- https://developers.openai.com/codex/app-server
- https://github.com/openai/codex/blob/main/codex-rs/app-server/README.md
- CLI `codex --version`, `codex app-server --help`, `codex login --help`, and
  `codex app-server generate-json-schema --experimental` from the installed pin.
- Pinned source `codex-rs/core/config.schema.json` and
  `codex-rs/core/src/tools/spec_plan.rs` (tag `rust-v0.160.0`) for tool suppression.
