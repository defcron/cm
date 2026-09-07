# cm TODO

Reviewed 2026-09-07 against the current source. This is a planning backlog, not a claim that every proposed feature is necessary or that every suspected failure has been reproduced. **Confirmed** means visible in the implementation; **Verify** means investigate with a focused test; **Enhancement** means optional product work. Effort estimates: S = small focused change, M = several related changes, L = broader design/integration work.

Keep cm a fast, non-interactive command that composes with shell tools. Keep Mirror as the conversation source of truth; do not introduce a local transcript cache by default.

## P1 — Reliability first

- [ ] **CM-01 · Confirmed · M — Fail correctly on streaming errors and incomplete responses.** `src/client.rs` ignores JSON error events and treats EOF as success even without `[DONE]`. Parse Mirror error envelopes, distinguish completed and interrupted streams, reject unexpected content types, and retain the previous thread state on failure. Acceptance: errors before/after partial output and truncated streams return nonzero with useful stderr; successful empty output remains distinguishable from failure.
- [ ] **CM-02 · Confirmed · M — Make SSE decoding safe across arbitrary byte boundaries.** `String::from_utf8_lossy` runs independently on each network chunk, which can corrupt a split UTF-8 character. Use incremental decoding and event framing; handle CRLF, comments, multiline data, and bounded buffers. Acceptance: splitting a fixture at every byte boundary produces identical text and conversation IDs, including emoji and non-Latin text.
- [ ] **CM-03 · Confirmed · M — Prevent concurrent invocations from losing thread state.** `src/state.rs` rewrites the whole state map using one `.state.json.tmp` filename without locking. Use a locked read/merge/write operation with a unique temporary file; serialize same-thread sends or reject overlap clearly. Acceptance: simultaneous different-thread calls preserve both IDs, same-thread calls have deterministic ordering, and interruption leaves readable state.
- [ ] **CM-04 · Confirmed · M — Define and test configuration precedence once.** `Config::load()` loads the working-directory dotenv before the advertised state-dir/binary-dir order; comments contradict one another. Choose explicit precedence, reconcile `CM_STATE_DIR` discovery, and update README/examples. Trim key values consistently and validate empty URLs/models and boolean settings. Test sources in isolated subprocesses so environment mutation cannot contaminate tests.
- [ ] **CM-05 · Confirmed · M — Establish automated regression tests and CI.** No authored tests or CI workflow were found in this checkout. Add a local mock HTTP server covering streaming/non-streaming, bearer-key fallback, fresh/continued threads, error exit codes, stdin, and state persistence. Add formatting, lint, and test checks; include the defects above as regression cases. CI must not need a real account or credentials.

## P2 — Make everyday use more dependable

- [ ] **CM-06 · Confirmed · M — Add connection and stream-idle timeouts.** The HTTP client has no explicitly configured timeouts. Provide configurable limits that accommodate long reasoning responses, plus a clear interruption message. Do not automatically retry a sent completion: it may already have created an upstream turn.
- [ ] **CM-07 · Confirmed · S — Handle output failures deliberately.** Streaming writes/flushes currently discard errors. Define clean behavior for broken pipes, propagate other write failures, and stop reading the response when its consumer disappears. Acceptance: `cm ... | head` does not silently keep consuming a full generation.
- [ ] **CM-08 · Enhancement · M — Add `cm doctor` or equivalent diagnostics.** Show the resolved server URL, configuration source names, state path, and model; probe health and authenticated model discovery without generating a chat. Never print keys, cookies, or token-bearing URLs. Explain common 401, connection, WARP, and session failures.
- [ ] **CM-09 · Enhancement · M — Support named server profiles.** Scope remembered conversation IDs to the server/profile so changing `CM_BASE_URL` cannot silently reuse an unrelated server's thread ID. Define a migration for the existing state file, and document importing/attaching an existing Mirror conversation ID.
- [ ] **CM-10 · Enhancement · S — Normalize base URLs and document installation.** Decide whether to accept both server-root and `/v1` base URLs; the current string concatenation would append a second `/v1`. Add clear build/install/PATH instructions, document the state location and `CM_STATE_DIR`, and explain that stored IDs are Mirror-local IDs rather than upstream ChatGPT IDs.
- [ ] **CM-11 · Enhancement · M — Improve thread management.** Add rename, explicit attach/resume, and local forget commands with clear help. Keep local forgetting distinct from deleting remote conversations. Warn when a successfully completed persistent request returns no usable conversation ID.

## P3 — Optional super-awesomer features

- [ ] **CM-12 · Enhancement · M — Script-friendly JSON output.** Add a structured output mode with reply, Mirror conversation ID, model, completion status, and optional metadata; keep diagnostics on stderr and avoid mixing text deltas with JSON.
- [ ] **CM-13 · Enhancement · M — Model/GPT discovery.** Add a command to list available models, GPTs, and Projects using Mirror's existing API, with filtering and copyable identifiers.
- [ ] **CM-14 · Enhancement · M — Temporary turns and attachments.** Consider explicit one-shot/private options and image-file input using Mirror's supported content format. Preserve the active thread when making a one-shot request, and document storage/privacy semantics precisely.
- [ ] **CM-15 · Enhancement · M — Easy installation and discoverability.** Add shell completions, a man page, versioned release binaries/checksums, and a tested upgrade path. Keep the existing plain stdout workflow as the default.

## Shared acceptance with Mirror

- [ ] **CM-16 · Verify · M — Maintain a two-project integration smoke test.** Exercise fresh streaming turn → separate process continuation → non-streaming turn → restart/reload → continuation; verify stable IDs and real context. Cover key aliases, server errors, disconnects, and wrong-server state. Use deterministic fixtures in CI and a small explicitly initiated live check for releases.

## Already working — preserve

- Streaming and non-streaming replies and persisted-ID continuation passed live checks in this task.
- `OPENAI_API_KEY` already works as cm's last-resort client key source; Mirror now accepts that server-side alias too.
- The server-side CORS/empty-origin fixes are completed work, not cm defects.

Suggested order: CM-01/02 with CM-05, then CM-03/04, then diagnostics and profiles. Reassess optional features after these reliability improvements land.
