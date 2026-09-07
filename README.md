# cm

See [TODO.md](TODO.md) for prioritized remaining work, verification gaps, and suggested improvements.

A small, non-interactive Rust CLI for chatting with GPTs through Mirror's
OpenAI-compatible `/v1/chat/completions` endpoint. No TUI, no REPL - just a
command you run each time, with the conversation persisting across runs by
default.

## The core idea

```
cm what's the capital of nowhere in particular
cm ok now argue the opposite case
```

The first call has no prior thread, so it starts a brand-new Mirror
conversation. The second call automatically continues it - same thread,
full context, no flags needed. Start a new topic with `-n`/`--new`:

```
cm -n let's talk about something else entirely
```

cm never stores your conversation history locally - it only remembers the
upstream Mirror **conversation id** for each thread (in a small state file)
and lets Mirror's `metadata.conversation_id` / `x-mirror-conversation-id`
threading handle the rest. Mirror is the source of truth.

## Setup

Build it:

```
cargo build --release
```

Configure it with a `.env` file (copy `.env.example`) or real env vars:

- `CM_BASE_URL` - your Mirror server, e.g. `http://localhost:8799`
- `CM_API_KEY` - explicit override for the bearer token cm sends, if you want cm to use a different key than the rest of Mirror. If unset, cm falls back in order to `MIRROR_API_KEY`, then the first entry in `MIRROR_API_KEYS` (Mirror's own comma-separated multi-key env var - same names the server itself reads), then `OPENAI_API_KEY` as a last resort. So a `.env` shared with your Mirror server's own `MIRROR_API_KEY`/`MIRROR_API_KEYS` just works with no duplication.
- `CM_MODEL` - default model/gizmo/project id (default `auto`)
- `CM_THREAD` - default thread name (default `default`)
- `CM_STREAM` - `false` to disable streaming by default

`.env` is looked for in (in order, each only filling gaps left by the last):
the state dir, next to the `cm` binary, and your current directory. Real
environment variables always take priority over any `.env` file.

## Usage

```
cm <message...>              send a message, continuing the default thread
cm -n <message...>            start a fresh conversation instead
cm -t work <message...>       use a separate named thread ("work")
cm -m gpt-5-6-thinking <msg>  override the model for this call
cm -s "You are terse." <msg> seed a new thread with a system prompt
cm --no-stream <message...>   print the full reply at once instead of streaming
cm --show-id <message...>     print the thread's conversation id to stderr
cm --reset                    forget the stored id for a thread (next call starts fresh)
cm --list-threads              list known threads and their conversation ids
echo "hi" | cm                 read the message from stdin instead of argv
```

Multiple named threads (`-t`/`--thread`) let you run several independent
persistent conversations side by side - e.g. `cm -t code ...` and
`cm -t brainstorm ...` never interfere with each other.

## How threading actually works

Every call sends only the new user message (plus, on a brand-new thread, an
optional system prompt) - never the full history - along with
`metadata.conversation_id` set to the thread's stored id, if any. Mirror
resolves that id server-side and keeps the real conversation state; cm just
reads `x-mirror-conversation-id` back off the response and remembers it for
next time. This keeps cm simple and keeps your actual conversation history
wherever Mirror (and ultimately chatgpt.com) already keeps it.
