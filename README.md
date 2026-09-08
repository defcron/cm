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

## Quick setup: unknown commands powered by cm

From this checkout, run the setup helper:

```bash
python3 scripts/setup-bash.py
source ~/.bashrc
```

Prerequisites: Python 3, Rust/Cargo, **Bash 4 or newer**, and access to a working
Mirror server. The helper builds cm with `cargo build --locked --release`,
installs `~/.local/bin/cm` and `~/.local/share/cm/shell/cm.bash`, and adds marked
blocks to `~/.bashrc` and `~/.bash_profile` to enable the integration in
interactive Bash sessions. Cargo needs dependencies available locally or over
the network.

Existing files are backed up before changes. Repeat runs update the installation
without duplicate startup blocks. If `.bash_profile` must be created, it retains
loading of the previously selected `.bash_login` or `.profile`. Setup does not
change your default login shell or configure server credentials; configure the
endpoint and authentication below before executing requests.

On macOS, `/bin/bash` is too old for the unknown-command hook. Install a current
Bash first; setup checks common Homebrew locations too. To select one explicitly:

```bash
python3 scripts/setup-bash.py --bash /opt/homebrew/bin/bash
/opt/homebrew/bin/bash --login
```

Use the path appropriate to your installation. Setup prints a command to start
the Bash it validated. Zsh and other shells do not load this integration. Use
`--skip-build` to install an already built `target/release/cm`.

## Why use unknown-command mode?

With `cm -e`, the model generates shell code and cm runs it. The Bash integration
also handles unknown commands: type the name of a tool you wish existed, with
its arguments, and cm asks the model to implement it, save it in `~/.local/bin`,
and run it.

This is useful for small utilities you would otherwise write by hand: inspecting
file formats, converting data, formatting output, or automating repetitive work.
A descriptive name and arguments become the specification. A successfully
installed command can subsequently run directly, without another model request
or copying code from a chat.

For example, assuming these commands are not already installed:

```bash
json-keys ./example.json
count-lines-by-extension ./src
```

Bash tries normal command lookup first. Only an unresolved command reaches
`command_not_found_handle`, which passes the command and quoted arguments to cm
in execution mode. The prompt requires a complete executable to be saved before
it runs. This depends on the model's response; cm does not enforce installation
with a separate file validator.

For more specific requirements, use an explicit request:

```bash
cm -e 'Create a command named json-keys that prints top-level JSON keys, save it, and run it on ./example.json'
```

**Execution mode runs generated code automatically, without a confirmation
step.** Enabling the hook applies that behavior to typos and other unresolved
commands too. Command text and arguments are sent to your configured
Mirror/model. Use ordinary chat mode when you want an explanation or code to
read before choosing to execute it.

## Shell behavior and output

The handler executes synchronously and returns the generated command's status.
Bash evaluates `&&`, `||`, `;`, `$?`, pipelines, and redirections in normal order:

```bash
json-keys ./example.json > keys.txt && printf 'Saved keys\n'
json-keys ./missing.json || printf 'Could not read JSON\n' >&2
```

The request uses a heredoc; Bash `%q` quoting preserves argument boundaries,
embedded newlines, quotes, and special characters. Execution happens outside the
heredoc, preserving original stdin, including piped input. Normal Bash parsing
still happens first: quote spaces and literal wildcards as for any command.

| Output | Stream |
| --- | --- |
| Generated command/script echoed before execution | stderr |
| Additional execution notice with `-v` / `--verbose` | stderr |
| Executed command's standard output | stdout |
| Executed command's errors | stderr |
| Ordinary chat replies without `-e` | stdout |

Put cm options before the message, for example `cm -e -v 'pwd'`.

The loaded `cm` Bash function sources explicit `cm -e` responses in the calling
shell, so top-level `cd`, variable assignments, and function definitions can
persist. Bash runs its unknown-command hook in a separate execution environment,
so shell-state changes there do not persist in the outer shell. Nothing is
queued for a later prompt. Pipelines and subshells retain normal Bash isolation.

Calling the executable directly, such as `command cm -e 'pwd'`, bypasses the
wrapper and uses a child shell. External scripts launched by generated code also
have ordinary external-command behavior.

## Updating or removing the integration

After changing or pulling source, rerun setup and reload Bash:

```bash
python3 scripts/setup-bash.py
source ~/.bashrc
```

The installation contains copies, so it does not depend on keeping the checkout
in the same location. Rerun setup to install newer copies. Rebuilding refreshes
the bundled execution prompt.

To remove the helper's startup blocks:

```bash
python3 scripts/setup-bash.py --uninstall
```

Then open a new terminal. Installed files and generated commands are retained;
setup removes only its marked startup blocks. Manually added source lines or
handlers must be removed separately. Earlier handlers in your configuration
become active again in a fresh shell.

## Configuration and manual build

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
- `CM_STATE_DIR` - override the directory containing conversation state, an optional `.env`, and optional `prompts.yaml`

For a quick session configuration:

```bash
export CM_BASE_URL=http://localhost:8799
# Set CM_API_KEY to your server's key if authentication is required.
```

For configuration that works from any directory, put a `.env` based on
[.env.example](.env.example) in the state directory:

- macOS: `~/Library/Application Support/net.eternalvoid.cm/`
- Linux: `${XDG_DATA_HOME:-$HOME/.local/share}/cm/`
- Any platform: the directory named by `CM_STATE_DIR`, when set.

The current loader reads a `.env` in the working directory or its ancestors
first, then the state directory, then next to the executable. Values already
set are retained, and real environment variables take priority. Installing cm
does not copy a checkout's `.env` or other credentials.

The default execution prompt is bundled from [prompts.yaml](prompts.yaml) when
cm is built. To customize it without rebuilding, put your own `prompts.yaml`
with an `exec: |` string in the state directory. `{shell}` is replaced with the
selected shell. A runtime override takes precedence over the bundled prompt.

## Usage

```
cm <message...>              send a message, continuing the default thread
cm -n <message...>            start a fresh conversation instead
cm -t work <message...>       use a separate named thread ("work")
cm -m gpt-5-6-thinking <msg>  override the model for this call
cm -s "You are terse." <msg> seed a new thread with a system prompt
cm --no-stream <message...>   print the full reply at once instead of streaming
cm --show-id <message...>     print the thread's conversation id to stderr
cm -e <request...>           generate and execute shell code
cm -e -v <request...>        also print the execution notice to stderr
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

## Development checks

```bash
cargo build --locked --release
python3 tests/parent_shell.py
python3 tests/setup_bash.py
```

The shell tests use a local mock API and isolated Bash sessions; they do not
contact a live model. Set `CM_TEST_BASH=/path/to/bash` to select the test shell.
The setup tests use temporary home directories.

## License

MIT. See [LICENSE](LICENSE).
