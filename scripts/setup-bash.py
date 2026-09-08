#!/usr/bin/env python3
"""Install cm and its opt-in Bash command-not-found integration for this user."""

import argparse
from datetime import datetime
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import tempfile


REPO = Path(__file__).resolve().parents[1]
BEGIN = "# >>> cm Bash integration >>>"
END = "# <<< cm Bash integration <<<"


def managed_text(original, block):
    lines = original.splitlines(keepends=True)
    starts = [i for i, line in enumerate(lines) if line.rstrip("\r\n") == BEGIN]
    ends = [i for i, line in enumerate(lines) if line.rstrip("\r\n") == END]
    if starts or ends:
        if len(starts) != 1 or len(ends) != 1 or starts[0] >= ends[0]:
            raise RuntimeError("malformed or duplicate cm startup markers; no startup files changed")
        return "".join(lines[:starts[0]]) + block + "".join(lines[ends[0] + 1:])
    if not block:
        return original
    separator = "" if not original or original.endswith("\n\n") else ("\n" if original.endswith("\n") else "\n\n")
    return original + separator + block


def write_file(path, content, mode=None):
    # Follow startup-file symlinks so existing dotfile-manager links stay intact.
    path = path.resolve()
    path.parent.mkdir(parents=True, exist_ok=True)
    existed = path.exists()
    if existed and path.read_bytes() == content:
        if mode is not None:
            path.chmod(mode)
        return
    if existed:
        stamp = datetime.now().strftime("%Y%m%d-%H%M%S-%f")
        backup = path.with_name(path.name + ".cm-backup-" + stamp)
        shutil.copy2(path, backup)
        print(f"Backup: {backup}")
    permissions = mode if mode is not None else (path.stat().st_mode & 0o777 if existed else 0o600)
    fd, temporary = tempfile.mkstemp(prefix=".cm-setup-", dir=path.parent)
    try:
        with os.fdopen(fd, "wb") as output:
            output.write(content)
        os.chmod(temporary, permissions)
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)
    print(f"Updated: {path}")


def find_bash(explicit):
    candidates = [explicit] if explicit else [shutil.which("bash"), "/opt/homebrew/bin/bash", "/usr/local/bin/bash", "/bin/bash"]
    for candidate in dict.fromkeys(candidates):
        if not candidate:
            continue
        try:
            check = subprocess.run(
                [candidate, "--noprofile", "--norc", "-c", 'test "${BASH_VERSINFO[0]:-0}" -ge 4'],
                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=5,
            )
            if check.returncode == 0:
                return str(Path(shutil.which(candidate) or candidate).resolve())
        except (OSError, subprocess.TimeoutExpired):
            continue
    raise RuntimeError("Bash 4+ is required for command_not_found_handle. Install a current Bash and rerun with --bash /path/to/bash.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bash", help="Bash 4+ executable to validate and use for activation instructions")
    parser.add_argument("--skip-build", action="store_true", help="install the existing target/release/cm binary")
    parser.add_argument("--uninstall", action="store_true", help="remove managed startup blocks; keep installed files and generated commands")
    args = parser.parse_args()
    home = Path.home()
    bashrc = home / ".bashrc"
    profile = home / ".bash_profile"
    rc_original = bashrc.read_text() if bashrc.exists() else ""
    profile_original = profile.read_text() if profile.exists() else ""

    if args.uninstall:
        edits = [(path, managed_text(original, "")) for path, original in ((bashrc, rc_original), (profile, profile_original))]
        for path, updated in edits:
            if path.exists():
                write_file(path, updated.encode())
        print("Removed managed startup blocks. Open a new terminal to stop using the loaded functions.")
        print("Installed cm files and commands in ~/.local/bin were retained.")
        return

    bash = find_bash(args.bash)
    rc_block = f'''{BEGIN}
if [[ $- == *i* && ${{BASH_VERSINFO[0]}} -ge 4 ]]; then
    export PATH="$HOME/.local/bin:$PATH"
    source "$HOME/.local/share/cm/shell/cm.bash"
    _CM_SETUP_ACTIVE=1
fi
{END}
'''
    # If creating .bash_profile, keep the startup file Bash previously selected.
    inherited = ""
    if not profile.exists():
        for previous in (home / ".bash_login", home / ".profile"):
            if previous.exists():
                inherited = f'# Preserve the previously selected Bash login configuration.\n[ ! -r "$HOME/{previous.name}" ] || . "$HOME/{previous.name}"\n\n'
                break
    profile_block = f'''{BEGIN}
if [[ $- == *i* && ${{BASH_VERSINFO[0]}} -ge 4 && ${{_CM_SETUP_ACTIVE:-0}} != 1 ]]; then
    [ ! -r "$HOME/.bashrc" ] || . "$HOME/.bashrc"
fi
{END}
'''
    # Validate all marker edits before installing or modifying anything.
    edits = [
        (bashrc, managed_text(rc_original, rc_block)),
        (profile, managed_text(inherited + profile_original, profile_block)),
    ]
    if not args.skip_build:
        subprocess.run(["cargo", "build", "--locked", "--release"], cwd=REPO, check=True)
    binary = REPO / "target/release/cm"
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise RuntimeError("target/release/cm is missing or not executable; run cargo build --release first")
    binary_target = home / ".local/bin/cm"
    # Refuse installation through a symlink rather than overwrite its target.
    integration_target = home / ".local/share/cm/shell/cm.bash"
    if binary_target.is_symlink() or integration_target.is_symlink():
        raise RuntimeError("an installation destination is a symlink; move it aside before installing")
    write_file(binary_target, binary.read_bytes(), 0o755)
    write_file(integration_target, (REPO / "shell/cm.bash").read_bytes(), 0o644)
    for path, updated in edits:
        write_file(path, updated.encode())
    print("\nInstalled cm and synchronous unknown-command handling.")
    print("Configure the Mirror endpoint and authentication before executing requests (see README.md).")
    print("In an existing Bash 4+ terminal, activate with: source ~/.bashrc")
    print(f"To start the validated Bash as a login shell: {shlex.quote(bash)} --login")
    print("The installer does not change your default login shell.")


if __name__ == "__main__":
    try:
        main()
    except (OSError, RuntimeError, subprocess.SubprocessError) as error:
        print(f"cm setup: {error}", file=sys.stderr)
        sys.exit(1)
