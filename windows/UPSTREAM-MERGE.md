# Upstream merge: ChatGPT account integration

Merge inputs: fork `a8e82ff70d07733385ea8207d2abd98c9234a685` and upstream
`71b773f08101237e1ec950efdaee17a626c830e6`. Work was done on
`update-upstream-chatgpt` in a separate worktree. The installed app was not changed.

## Conflict decisions

All paths below are relative to `windows/`.

| Conflicted file / hunk | Classification | Resolution |
| --- | --- | --- |
| `package.json`: test scripts | Disjoint | Keep upstream `npm test` and account-chat Edge smoke tests, the Codex preparation lifecycle, upstream version and dependencies. |
| `package-lock.json`: both version fields | Superseded/generated | Regenerate with `npm install` from the combined manifest, retaining the exact Codex 0.160.0 pin. |
| `src-tauri/Cargo.toml`: Tokio features | Disjoint | Union `process` (Codex) and `macros` (upstream), retaining all phone-link and shortcut dependencies. Cargo re-resolved the combined workspace without dependency upgrades. |
| `src-tauri/src/lib.rs`: module declarations and managed state | Disjoint | Keep Codex account state alongside upstream plan, desktop, shortcut and recap modules/state. |
| `src-tauri/src/lib.rs`: settings save | Same question, different answers | Reset both backend conversations only when crossing into/out of ChatGPT. Preserve upstream plain-history switching between ordinary API/local providers. Emit `chat-cleared` so even an optimistically changed picker clears its UI. Keep upstream screen, autostart, shortcuts, language, integrations, Spotify and Rust-owned phone/desktop preferences. |
| `src-tauri/src/lib.rs`: chat send/state parameter | Superseded + disjoint | Dispatch `chatgpt` to the isolated account client; all other providers go through upstream `chat::send`. Retain upstream model discovery/local-server commands and account commands. |
| `src-tauri/src/settings.rs`: enum, struct, defaults | Superseded | Use upstream extensible string provider and complete settings schema, preserving the serialized `chatgpt` value. Replace the old two-provider migration test with legacy/default and all-provider round-trip coverage. |
| `src/core/state.ts`: provider type and defaults | Superseded | Use upstream `ProviderId`, settings and defaults; add `chatgpt` to the provider registry rather than restoring a two-provider type. |
| `src/core/bridge.ts`: reset and provider commands | Disjoint + same question | Keep both sets of commands. New chat uses throwing IPC so failures are not presented as successful resets. |
| `src/main.ts`: clear vs sound event | Disjoint | Keep separate handlers for both. Clear attachments/window context along with account conversation history. |
| `src/main.ts`: settings handler | Superseded | Retain upstream language/pill-focus handling; backend `chat-cleared` controls account-boundary resets instead of clearing on every API-provider change. |
| `src/settings/main.ts`: API-key hints | Same question | Retain localized, platform-specific credential-store wording and clarify Anthropic-only key requirements. |
| `src/settings/main.ts`: sections | Disjoint | Keep account controls and all upstream agent, plan, cloud/local-provider, phone, shortcut and other settings sections. Account selector lists the combined provider registry. |
| `src/views/chat.ts`: buttons and sending state | Disjoint | Preserve localized Send, upstream Markdown, streaming and model picker; retain New chat, failed-turn rollback/retry and stale-provider result guard. |

## Necessary integration edits outside conflict blocks

- `src-tauri/src/codex.rs`: only change references from `claude::ChatContext/ChatReply`
  to upstream's `chat::ChatContext/ChatReply`. Authentication, environment allowlist,
  no-tool/no-environment controls, read-only sandbox, timeouts and redaction are unchanged.
- `src/core/providers.ts`, `src/views/chat.ts`: ChatGPT is distinct from OpenAI API;
  its picker checks account status, never asks for an API key or model list, and
  explicitly reports that Codex chooses its model.
- `src/core/phone-chat.ts`: exclude ChatGPT account IDs from phone model helpers;
  upstream Rust phone chat already accepts only its API/local providers.
- `scripts/gen-strings.mjs`, `src/i18n/strings.json`: upstream's generated table was
  stale against its current Mac catalog. Regeneration exposed untranslated brand
  names and unfinished Mac-only entries shadowing complete Windows translations.
  Skip incomplete catalog entries, regenerate, and retain strict Windows used-key
  coverage. A regression test first reproduced the incomplete-entry problem.
- `scripts/test-chatgpt-ui.mjs`: Windows rejected the old fixed loopback port
  with `EACCES` (confirmed by both Vite startup output and a direct socket bind).
  The harness now awaits an OS-assigned loopback listener, surfaces startup errors,
  and closes Vite explicitly. Two consecutive real Edge smoke runs passed on
  different assigned ports; no test was waived. It also checks all eight settings
  provider choices rather than only the original pair.
- Frontend tests cover the expanded registry, account-only picker and phone
  exclusion; existing chat tests now select the Send button explicitly rather
  than accidentally clicking the newly retained New chat button.

## Verification scope

Final verification completed before committing:

- `cargo test --workspace --locked`: 416 library tests passed, 0 failed, 2 ignored;
  30 hook tests passed, 0 failed. Binary/doc targets each had 0 tests.
- `npm test`: 400 passed, 0 failed, 0 skipped.
- `npm run test:chatgpt-ui`: passed; real headless Edge with mocked Tauri IPC.
  Repeated successful runs used distinct OS-assigned ports after the bind fix.
- `node scripts/gen-strings.mjs --check`: up to date.
- `npm run build`: TypeScript and Vite passed (74 transformed modules).
- `npm run pack`: final-state rebuild passed; both NSIS and MSI produced.
  `release/Coucou-Windows-setup.exe`: 99,441,720 bytes;
  `release/Coucou-Windows.msi`: 144,453,632 bytes. Versioned copies also exist.
  Hash comparisons verified release copies match their Tauri bundle outputs.
- Bundled `target/release/codex/codex.exe --version`: `codex-cli 0.160.0`;
  its hash matches the prepared pinned resource; license, notice and README exist.
- Added-line security scan found no secret/injection matches.
  `git diff --check origin/main` passed; a full diff against the older fork also
  contains upstream's pre-existing trailing blank line in `docs/ANDROID_LINK.md`.

Local execution logs are retained under `windows/target/merge-verification/`
(ignored build output), not committed. No installer was run and nothing was pushed.

The Codex tests launch the real pinned CLI with isolated temporary profiles and
synthetic local model responses; all captured requests must have an empty tools
array. They also exercise signed-out status, browser-OAuth initiation/cancellation,
logout verification, history, errors and timeout behavior. The Edge test mocks
Tauri IPC. Neither test authenticates a user's account or obtains a live OpenAI
model answer. Android/Kotlin relay interop tests are separately opt-in. Linux,
macOS and Android builds are not validated by this Windows merge exercise; the
account integration remains Windows x64 only. Its new account-specific UI text
is English, as before this merge; upstream translated UI remains intact.
