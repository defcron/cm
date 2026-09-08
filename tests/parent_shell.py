"""Run after cargo build --release; uses a local mock API and isolated Bash."""
import http.server
import json
import os
from pathlib import Path
import pty
import select
import shlex
import shutil
import signal
import subprocess
import tempfile
import threading
import time


REPO = Path(__file__).resolve().parents[1]
BASH = os.environ.get("CM_TEST_BASH") or shutil.which("bash")
if Path("/opt/homebrew/bin/bash").exists() and "CM_TEST_BASH" not in os.environ:
    BASH = "/opt/homebrew/bin/bash"
INTEGRATION = shlex.quote(str(REPO / "shell/cm.bash"))


def main():
    with tempfile.TemporaryDirectory(prefix="cm-parent-test-") as tmp:
        root = Path(tmp).resolve()
        tmp = str(root)
        destination = root / "directory with spaces"
        destination.mkdir()
        received = []
        semantics = {
            "ok": "printf 'ok-output\\n'\ntrue",
            "fail": "printf 'failure-output\\n'\n(exit 7)",
            "input": "cat",
        }
        for name, script in semantics.items():
            path = root / f"regular_semantics_{name}"
            path.write_text(f"#!{BASH}\n{script}\n")
            path.chmod(0o700)
        (root / "input-file").write_text("file input\n")

        class Handler(http.server.BaseHTTPRequestHandler):
            def do_POST(self):
                request = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
                message = request["messages"][-1]["content"]
                received.append(message)
                if message.startswith("api-failure"):
                    self.send_error(500, "Test failure")
                    return
                if message.startswith("return-seven"):
                    script = "return 7"
                elif message.startswith("generated-missing"):
                    script = "cm_generated_unavailable_command"
                elif message.startswith("plain-chat"):
                    script = "chat reply"
                elif message.startswith("cm_semantics_"):
                    script = semantics[message.split()[0].removeprefix("cm_semantics_")]
                else:
                    script = (
                        f"cd {shlex.quote(str(destination))}\n"
                        "export CM_PARENT_PROOF='persisted value'\n"
                        "printf 'child-stdout\\n'\n"
                        "printf 'child-stderr\\n' >&2"
                    )
                body = json.dumps({"choices": [{"message": {"content": script}}]}).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.end_headers()
                self.wfile.write(body)

            def log_message(self, *_):
                pass

        server = http.server.HTTPServer(("127.0.0.1", 0), Handler)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        env = {
            "PATH": f"{root}:{REPO / 'target/release'}:/usr/bin:/bin",
            "HOME": tmp,
            "TMPDIR": tmp,
            "SHELL": BASH,
            "CM_STATE_DIR": str(root / "state"),
            "CM_BASE_URL": f"http://127.0.0.1:{server.server_port}",
            "CM_STREAM": "false",
            "PS1": "CM_TEST_PROMPT> ",
            "TERM": "dumb",
        }

        def run(source):
            return subprocess.run(
                [BASH, "--noprofile", "--norc", "-c", f"source {INTEGRATION}\n{source}"],
                cwd=tmp, env=env, text=True, capture_output=True, timeout=15,
            )

        try:
            for flag in ("", "-v", "--verbose"):
                result = run(
                    f"cm -e {flag} direct-test\n"
                    "printf 'parent:%s:%s\\n' \"$PWD\" \"$CM_PARENT_PROOF\""
                )
                assert result.returncode == 0, result
                assert result.stdout == f"child-stdout\nparent:{destination}:persisted value\n", result
                assert "cd " in result.stderr and "child-stderr\n" in result.stderr, result
                assert ("cm: preparing" in result.stderr) == bool(flag), result
            assert "sourced in the user's current Bash shell" in received[-1]
            assert "REQUIRED PERSISTENCE: Every new command or script" in received[-1]
            assert "cm does not automatically save your response" in received[-1]
            print("PASS: parent cd/export persist; command echo/errors use stderr; output uses stdout")
            print("PASS: execution request contains bundled prompts.yaml persistence instructions")

            result = run("cm -e return-seven")
            assert result.returncode == 7, result
            result = run("cm -e api-failure\nprintf 'status:%s:%s\\n' \"$?\" \"${CM_PARENT_PROOF-unset}\"")
            assert result.stdout == "status:1:unset\n", result
            before = len(received)
            result = run("cm -e generated-missing")
            assert result.returncode == 127 and len(received) == before + 1, result
            result = run("cm plain-chat")
            assert result.stdout == "chat reply\n" and not result.stderr, result
            result = run("cm -e direct-test | cat\nprintf 'outer:%s:%s\\n' \"$PWD\" \"${CM_PARENT_PROOF-unset}\"")
            assert result.stdout == f"child-stdout\nouter:{tmp}:unset\n", result
            assert not list(root.glob("cm-source.*"))
            print("PASS: status preserved; failed requests do not execute; no recursive generation; chat unchanged")

            cases = [
                "CMD_ok && printf 'and-ran\\n'",
                "CMD_fail && printf 'must-not-run\\n'; printf 'status:%s\\n' \"$?\"",
                "CMD_fail || printf 'or-ran:%s\\n' \"$?\"",
                "CMD_ok || printf 'must-not-run\\n'; printf 'status:%s\\n' \"$?\"",
                "CMD_fail; printf 'semicolon:%s\\n' \"$?\"",
                "if CMD_fail; then printf 'bad\\n'; else printf 'else:%s\\n' \"$?\"; fi",
                "! CMD_fail; printf 'negated:%s\\n' \"$?\"",
                "printf 'pipe input\\n' | CMD_input",
                "CMD_input < input-file > output-file; cat output-file",
                "set -o pipefail; CMD_fail | cat; printf 'pipefail:%s\\n' \"$?\"",
                "CMD_fail | cat; printf 'pipestatus:%s\\n' \"${PIPESTATUS[*]}\"",
                "captured=$(CMD_ok); printf 'captured:%s\\n' \"$captured\"",
                "CMD_fail & job=$!; wait \"$job\"; printf 'wait:%s\\n' \"$?\"",
                "set -e; CMD_fail; printf 'must-not-run\\n'",
            ]
            for case in cases:
                actual = run(case.replace("CMD_", "cm_semantics_"))
                regular = run(case.replace("CMD_", "regular_semantics_"))
                assert (actual.stdout, actual.returncode) == (regular.stdout, regular.returncode), (case, actual, regular)
            print(f"PASS: {len(cases)} semantic comparisons against regular executables, including stdin, pipelines and errexit")

            # Reloading must remove the old queue hook without losing other hooks.
            for setup in [
                "PROMPT_COMMAND='_cm_run_pending; kept-hook'",
                "PROMPT_COMMAND=('_cm_run_pending' 'kept-hook')",
            ]:
                result = run(setup + f"\nsource {INTEGRATION}\ndeclare -p PROMPT_COMMAND")
                assert "_cm_run_pending" not in result.stdout and "kept-hook" in result.stdout, result
            print("PASS: obsolete string/array prompt hooks removed; unrelated hooks retained")

            override = root / "state/prompts.yaml"
            override.write_text("exec: |\n  CUSTOM_RUNTIME_PROMPT {shell}\n")
            try:
                result = run("cm -e return-seven")
                assert result.returncode == 7, result
                assert f"CUSTOM_RUNTIME_PROMPT {BASH}" in received[-1], received[-1]
                assert "REQUIRED PERSISTENCE" not in received[-1], received[-1]
            finally:
                override.unlink()
            print("PASS: runtime prompts.yaml override still takes precedence")

            # Exercise Bash's real interactive missing-command dispatch.
            pid, fd = pty.fork()
            if pid == 0:
                os.chdir(tmp)
                os.execve(BASH, [BASH, "--noprofile", "--norc", "-i"], env)

            def until_prompt():
                data = b""
                deadline = time.monotonic() + 15
                while b"CM_TEST_PROMPT> " not in data:
                    assert time.monotonic() < deadline, data
                    if select.select([fd], [], [], 0.2)[0]:
                        data += os.read(fd, 65536)
                return data.decode(errors="replace")

            def terminal(command):
                os.write(fd, (command + "\n").encode())
                return until_prompt()

            try:
                until_prompt()
                terminal(f"PROMPT_COMMAND=('CM_EXISTING_HOOK=kept'); source {INTEGRATION}; source {INTEGRATION}")
                output = terminal("cm_parent_missing 'two words' $'line one\\nline two' '$HOME' 'CM_EOF'")
                assert "child-stdout" in output, output
                output = terminal("printf 'parent:%s:%s:%s\\n' \"$PWD\" \"$CM_PARENT_PROOF\" \"$CM_EXISTING_HOOK\"")
                assert f"parent:{tmp}::kept" in output, output
                message = next(m for m in received if m.startswith("cm_parent_missing"))
                first_line = message.splitlines()[0]
                decoded = subprocess.run(
                    [BASH, "--noprofile", "--norc", "-c", 'eval "set -- $1"; printf "%s\\0" "$@"', "test", first_line],
                    env=env, capture_output=True, check=True,
                )
                assert decoded.stdout.split(b"\0")[:-1] == [
                    b"cm_parent_missing", b"two words", b"line one\nline two", b"$HOME", b"CM_EOF",
                ], decoded
                assert not list(root.glob("cm-pending.*/pending.*"))
                output = terminal("cm_semantics_fail && printf 'BAD\\n' || printf 'or-status:%s\\n' \"$?\"; printf 'after\\n'")
                assert "or-status:7\r\nafter\r\n" in output, output
                assert "\r\nBAD\r\n" not in output, output
                output = terminal("cm_semantics_ok && printf 'and-ran\\n'; printf 'status:%s\\n' \"$?\"")
                assert "and-ran\r\nstatus:0\r\n" in output, output
                terminal(f"cd {shlex.quote(tmp)}; unset CM_PARENT_PROOF")
                terminal("cm_redirected_missing > redirected-output")
                assert (root / "redirected-output").read_text() == "child-stdout\n"
                output = terminal("printf 'outer:%s:%s\\n' \"$PWD\" \"${CM_PARENT_PROOF-unset}\"")
                assert f"outer:{tmp}:unset" in output, output
                assert not list(root.glob("cm-pending.*/pending.*"))
                output = terminal("declare -p PROMPT_COMMAND")
                assert "_cm_run_pending" not in output, output
                assert "CM_EXISTING_HOOK=kept" in output, output
                print("PASS: interactive unknown commands execute synchronously; statuses, heredoc args and redirections preserved")
            finally:
                os.kill(pid, signal.SIGTERM)
                os.close(fd)
                os.waitpid(pid, 0)
        finally:
            server.shutdown()


if __name__ == "__main__":
    main()
