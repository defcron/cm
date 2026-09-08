"""Exercise installation, activation, repeat runs and removal in temporary homes."""

import http.server
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import threading


REPO = Path(__file__).resolve().parents[1]
SETUP = REPO / "scripts/setup-bash.py"
BASH = os.environ.get("CM_TEST_BASH") or shutil.which("bash")
if Path("/opt/homebrew/bin/bash").exists() and "CM_TEST_BASH" not in os.environ:
    BASH = "/opt/homebrew/bin/bash"


def main():
    class Handler(http.server.BaseHTTPRequestHandler):
        def do_POST(self):
            request = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            assert "REQUIRED PERSISTENCE" in request["messages"][-1]["content"]
            body = json.dumps({"choices": [{"message": {"content": "printf 'installed-output\\n'; (exit 7)"}}]}).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *_):
            pass

    server = http.server.HTTPServer(("127.0.0.1", 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        with tempfile.TemporaryDirectory(prefix="cm setup test ") as tmp:
            home = Path(tmp).resolve()
            env = {
                "HOME": str(home), "PATH": "/usr/bin:/bin", "SHELL": BASH,
                "CM_BASE_URL": f"http://127.0.0.1:{server.server_port}",
                "CM_STATE_DIR": str(home / "state"), "TERM": "dumb",
            }
            rc = home / ".bashrc"
            original = "export CM_EXISTING_RC=kept\n"
            rc.write_text(original)
            (home / ".profile").write_text("export CM_EXISTING_PROFILE=kept\n")

            def setup(*args):
                return subprocess.run(
                    [sys.executable, str(SETUP), "--skip-build", "--bash", BASH, *args],
                    cwd=home, env=env, capture_output=True, text=True, timeout=20,
                )

            first = setup()
            assert first.returncode == 0, first
            binary = home / ".local/bin/cm"
            integration = home / ".local/share/cm/shell/cm.bash"
            assert binary.read_bytes() == (REPO / "target/release/cm").read_bytes()
            assert os.access(binary, os.X_OK)
            assert integration.read_bytes() == (REPO / "shell/cm.bash").read_bytes()
            assert list(home.glob(".bashrc.cm-backup-*"))[0].read_text() == original
            assert '"$HOME/.profile"' in (home / ".bash_profile").read_text()

            before = {p: p.read_bytes() for p in (rc, home / ".bash_profile")}
            backups = set(home.rglob("*.cm-backup-*"))
            second = setup()
            assert second.returncode == 0, second
            assert all(p.read_bytes() == content for p, content in before.items())
            assert set(home.rglob("*.cm-backup-*")) == backups
            print("PASS: installation, backups, homes with spaces, preserved login config and idempotent reruns")

            # Load the installed files, using only installed cm on PATH.
            result = subprocess.run(
                [BASH, "--noprofile", "--norc", "-ic",
                 'source "$HOME/.bash_profile"; '
                 'printf "loaded:%s:%s:%s:%s\\n" "$(type -t cm)" "$(type -t command_not_found_handle)" "$CM_EXISTING_RC" "$CM_EXISTING_PROFILE"; '
                 'setup_missing_command && printf "BAD\\n" || printf "status:%s\\n" "$?"'],
                cwd=home, env=env, capture_output=True, text=True, timeout=15,
            )
            assert result.returncode == 0, result
            assert result.stdout == "loaded:function:function:kept:kept\ninstalled-output\nstatus:7\n", result
            print("PASS: installed executable and shell integration handle an unknown command with correct output/status")

            removal = setup("--uninstall")
            assert removal.returncode == 0, removal
            assert "# >>> cm" not in rc.read_text()
            assert "# >>> cm" not in (home / ".bash_profile").read_text()
            assert rc.read_text().strip() == original.strip()
            assert binary.exists() and integration.exists()
            assert setup("--uninstall").returncode == 0
            print("PASS: uninstall removes only managed blocks and can be repeated")

        with tempfile.TemporaryDirectory(prefix="cm-setup-invalid-") as tmp:
            home = Path(tmp)
            rc = home / ".bashrc"
            invalid = "# >>> cm Bash integration >>>\nmissing end marker\n"
            rc.write_text(invalid)
            result = subprocess.run(
                [sys.executable, str(SETUP), "--skip-build", "--bash", BASH],
                env={"HOME": tmp, "PATH": "/usr/bin:/bin"}, capture_output=True, text=True,
            )
            assert result.returncode != 0 and rc.read_text() == invalid, result
            assert not (home / ".local").exists()
            print("PASS: malformed startup markers rejected before installation or startup edits")
    finally:
        server.shutdown()


if __name__ == "__main__":
    main()
