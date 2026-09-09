mod client;
mod config;
mod state;

use anyhow::{bail, Result};
use clap::Parser;
use std::io::{IsTerminal, Read, Write};
use std::process::Command;

use config::Config;
use state::StateFile;

/// cm - a non-interactive CLI for persistent GPT conversations against a
/// Mirror (chatgpt-mirror) OpenAI-compatible chat-completions API.
///
/// By default, the first run starts a new conversation thread and every
/// run after that continues it - no flags needed. Pass -n/--new to start a
/// fresh thread deliberately (e.g. to change topic).
#[derive(Parser, Debug)]
#[command(name = "cm", version, about, long_about = None)]
struct Cli {
    /// The message to send. If omitted, cm reads the message from stdin
    /// (so `echo "hi" | cm` and `cm < prompt.txt` both work).
    #[arg(trailing_var_arg = true)]
    message: Vec<String>,

    /// Start a brand-new conversation thread instead of continuing the
    /// previous one. Only affects the named thread (see --thread).
    #[arg(short = 'n', long)]
    new: bool,

    /// Named thread to use, so you can keep several independent persistent
    /// conversations going at once. Defaults to CM_THREAD or "default".
    #[arg(short = 't', long)]
    thread: Option<String>,

    /// Model (or gizmo/project id) to use for this message. Defaults to
    /// CM_MODEL or "auto".
    #[arg(short = 'm', long)]
    model: Option<String>,

    /// System prompt to seed a brand-new conversation with. Only sent when
    /// this call actually starts a new thread (first run, or with --new).
    #[arg(short = 's', long)]
    system: Option<String>,

    /// Disable streaming output; print the full reply once it's ready.
    #[arg(long)]
    no_stream: bool,

    /// Print the thread's conversation id to stderr after the reply.
    #[arg(long)]
    show_id: bool,

    /// Treat the assistant response as shell commands: print it to stderr and execute it
    /// immediately using the current user's $SHELL without prompting.
    #[arg(short = 'e', long)]
    exec: bool,

    /// Print additional execution diagnostics to stderr.
    #[arg(short = 'v', long)]
    verbose: bool,

    /// Write execution-mode source for the Bash integration instead of spawning a shell.
    #[arg(long, hide = true)]
    shell_output: Option<std::path::PathBuf>,

    /// Forget the stored conversation id for a thread (use --thread to pick
    /// which one; defaults to the default thread) and exit without sending
    /// anything. Equivalent to guaranteeing the *next* call starts fresh.
    #[arg(long)]
    reset: bool,

    /// List all known threads and their stored conversation ids, then exit.
    #[arg(long)]
    list_threads: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = Config::load()?;
    let thread = cli.thread.clone().unwrap_or_else(|| cfg.thread.clone());

    // Serialize all state read/modify/write operations across concurrent cm invocations.
    let _state_lock = state::lock(&cfg.state_dir, &thread)?;
    let mut state = StateFile::load(&cfg.state_dir)?;

    if cli.list_threads {
        let threads = state.list_threads();
        if threads.is_empty() {
            eprintln!("(no threads yet)");
        } else {
            for (name, id) in threads {
                println!("{name}\t{}", id.unwrap_or("(new)"));
            }
        }
        return Ok(());
    }

    if cli.reset {
        state.clear(&thread);
        state.save(&cfg.state_dir)?;
        eprintln!("cm: cleared thread '{thread}' - next message starts a new conversation");
        return Ok(());
    }

    let message = if !cli.message.is_empty() {
        cli.message.join(" ")
    } else {
        let mut input = String::new();
        if std::io::stdin().is_terminal() {
            bail!(
                "no message given and stdin is a terminal - pass a message, e.g. `cm what's up`, or pipe one in"
            );
        }
        std::io::stdin()
            .read_to_string(&mut input)
            .map_err(|e| anyhow::anyhow!("reading stdin: {e}"))?;
        input.trim().to_string()
    };
    if message.is_empty() {
        bail!("empty message - nothing to send");
    }

    let mut existing_conversation_id = if cli.new {
        None
    } else {
        state.conversation_id(&thread).map(|s| s.to_string())
    };
    let is_new_thread = existing_conversation_id.is_none();

    // Mirror's API starts spewing a lot of automatic first-turn output (file
    // loading/searching, etc.) when a brand-new conversation is opened
    // against a Custom GPT/gizmo or a Project. In -e/--exec mode that output
    // would get executed as shell commands, which we don't want. So when
    // we're both starting a new thread AND running with -e/--exec, do a
    // harmless priming turn first (just saying "Hello") to absorb that
    // automatic output, then send the real message as a follow-up in the
    // same (now-established) conversation.
    if is_new_thread && cli.exec {
        if cli.verbose {
            eprintln!(
                "cm: starting a new conversation with -e/--exec - sending an initial \"Hello\" turn first to absorb the API's automatic first-turn output"
            );
        }
        // No system prompt here - it belongs on the real message below,
        // localized to the turn where -e's execution instructions actually
        // live, not on this throwaway priming turn.
        let priming = client::send_message(
            &cfg,
            "Hello",
            None,
            None,
            cli.model.as_deref(),
            false,
        )
        .await?;
        if let Some(id) = priming.conversation_id {
            existing_conversation_id = Some(id);
        }
    }

    let message = if cli.exec {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "unknown".to_string());
        let prompts = config::Prompts::load(&cfg.state_dir)?;
        let execution_context = if cli.shell_output.is_some() {
            "\n\nThe generated program will be sourced in the user's current Bash shell. Preserve requested changes to the working directory, variables, and functions. For routine failure use return, not exit; invoke programs normally rather than using exec. Do not exit or replace the interactive shell unless explicitly requested."
        } else {
            ""
        };
        format!("{}\n\n{}{}", message, prompts.exec.replace("{shell}", &shell), execution_context)
    } else {
        message
    };

    // Streaming is disabled in exec mode so only complete responses are executed.
    let stream = !cli.exec && !cli.no_stream && cfg.stream;

    let result = client::send_message(
        &cfg,
        &message,
        if is_new_thread {
            cli.system.as_deref()
        } else {
            None
        },
        existing_conversation_id.as_deref(),
        cli.model.as_deref(),
        stream,
    )
    .await?;

    if cli.exec {
        eprintln!("{}", result.reply);
        std::io::stderr().flush().ok();

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string());
        if cli.verbose {
            if cli.shell_output.is_some() {
                eprintln!("cm: preparing assistant response for the current Bash shell");
            } else {
                eprintln!("cm: executing assistant response with {shell}");
            }
        }
        if cli.shell_output.is_none() {
            Command::new(&shell)
                .arg("-c")
                .arg(&result.reply)
                .status()
                .map_err(|e| anyhow::anyhow!("executing shell command: {e}"))?;
        }
    } else {
        if !stream {
            println!("{}", result.reply);
        } else if !result.reply.ends_with('\n') {
            println!();
        }
        std::io::stdout().flush().ok();
    }

    if let Some(id) = &result.conversation_id {
        state.set_conversation_id(&thread, id.clone());
        state.save(&cfg.state_dir)?;
        if cli.show_id {
            eprintln!("cm: thread '{thread}' -> {id}");
        }
    }

    // Publish source only after the request and state persistence have succeeded.
    if cli.exec {
        if let Some(path) = &cli.shell_output {
            std::fs::write(path, &result.reply)?;
        }
    }

    Ok(())
}
