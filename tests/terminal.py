"""Run with Python 3, Bash 5.1+, and tmux: python3 tests/terminal.py BINARY."""

import contextlib
import os
from pathlib import Path
import re
import shlex
import signal
import subprocess
import sys
import tempfile
import time
import unicodedata
import uuid

BINARY = Path(sys.argv[1]).resolve()
HOOK = Path(__file__).resolve().parents[1] / "scripts/shell-glimmer.bash"


def cells(capture):
    style = dict(fg="default", bg="default", bold=False)
    result = []
    for row, line in enumerate(capture.splitlines()):
        column = 0
        index = 0
        while index < len(line):
            match = re.match(r"\x1b\[([0-9;:]*)m", line[index:])
            if match:
                numbers = [int(n or "0") for n in match[1].replace(":", ";").split(";")]
                k = 0
                while k < len(numbers):
                    code = numbers[k]
                    if code == 0:
                        style = dict(fg="default", bg="default", bold=False)
                    elif code in (1, 22):
                        style["bold"] = code == 1
                    elif code in (39, 49):
                        style["fg" if code == 39 else "bg"] = "default"
                    elif 30 <= code <= 37:
                        style["fg"] = ("index", code - 30)
                    elif 90 <= code <= 97:
                        style["fg"] = ("index", code - 90 + 8)
                    elif 40 <= code <= 47:
                        style["bg"] = ("index", code - 40)
                    elif 100 <= code <= 107:
                        style["bg"] = ("index", code - 100 + 8)
                    elif code in (38, 48) and k + 1 < len(numbers):
                        key = "fg" if code == 38 else "bg"
                        if numbers[k + 1] == 5:
                            style[key] = ("index", numbers[k + 2])
                            k += 2
                        elif numbers[k + 1] == 2:
                            style[key] = ("rgb", *numbers[k + 2:k + 5])
                            k += 4
                    k += 1
                index += len(match[0])
                continue
            char = line[index]
            index += 1
            if unicodedata.category(char) in ("Mn", "Me"):
                if result and result[-1][0] == row:
                    last = result[-1]
                    result[-1] = (*last[:2], last[2] + char, last[3])
                continue
            if not char.isspace():
                result.append((row, column, char, tuple(style.items())))
            column += 2 if unicodedata.east_asian_width(char) in ("W", "F") else 1
    return result


class Pane:
    def __init__(self, socket, home):
        self.socket = socket
        self.home = home

    def tmux(self, *args):
        result = subprocess.run(["tmux", "-L", self.socket, *args], capture_output=True, text=True, check=True)
        return result.stdout

    def capture(self):
        return self.tmux("capture-pane", "-p", "-e", "-t", "test")

    def state(self):
        return self.tmux("display-message", "-p", "-t", "test", "#{alternate_on} #{cursor_flag}").strip()

    def cursor(self):
        return self.tmux("display-message", "-p", "-t", "test", "#{cursor_x} #{cursor_y}").strip()

    def send(self, command):
        self.tmux("send-keys", "-t", "test", "-l", command)
        self.tmux("send-keys", "-t", "test", "Enter")

    def wait(self, predicate):
        deadline = time.monotonic() + 4
        while time.monotonic() < deadline:
            value = predicate()
            if value:
                return value
            time.sleep(0.008)
        raise AssertionError(("terminal did not reach expected state", self.state(), self.capture()))

    def check_cleanup(self):
        state = self.home / "state"
        self.send("{ declare -p PROMPT_COMMAND; declare -F; } > " + shlex.quote(str(state)))
        self.wait(lambda: state.exists() and state.read_text())
        assert "_glimmer" not in state.read_text(), state.read_text()
        assert "declare -a PROMPT_COMMAND=" in state.read_text(), state.read_text()


@contextlib.contextmanager
def pane(prompt=r"PS1$ ", *, source=True, width=100, height=30, x=0, y=7, mode="native", effect="assemble", setup="", last=":", binary=BINARY):
    with tempfile.TemporaryDirectory(prefix="shimmer-test-") as directory:
        home = Path(directory)
        rc = home / "rc.bash"
        environment = ""
        if mode == "relative":
            environment = "unset TMUX TMUX_PANE\n"
        elif mode == "stale":
            environment = "TMUX_PANE=%999999\n"
        hooks = "PROMPT_COMMAND=([2]='printf \"HOOK_RAN\\n\"; true' [9]=" + shlex.quote(f'printf "\\033[{y + 1};{x + 1}H"; false') + ")\n"
        rc.write_text(
            "HISTFILE=/dev/null\nPS1=" + shlex.quote(prompt) + "\n"
            'printf "\\033[2J\\033[HKEEP_BEFORE\\n"\n'
            + environment + hooks + setup + "\n"
            + ("source " + shlex.quote(str(HOOK)) + "\n" if source else "")
            + last + "\n"
        )
        instance = Pane("shimmer-test-" + uuid.uuid4().hex, home)
        instance.tmux("-f", "/dev/null", "new-session", "-d", "-s", "test", "-x", str(width), "-y", str(height),
                      "-e", "HOME=" + str(home), "-e", "TERM=tmux-256color",
                      "-e", "SHELL_GLIMMER_BIN=" + str(binary), "-e", "SHELL_GLIMMER_EFFECT=" + effect,
                      "bash --noprofile --rcfile " + shlex.quote(str(rc)) + " -i")
        try:
            yield instance
        finally:
            subprocess.run(["tmux", "-L", instance.socket, "kill-server"], capture_output=True)


def landing(name, prompt, **options):
    with pane(prompt, source=False, **options) as reference:
        reference.wait(lambda: "HOOK_RAN" in reference.capture())
        time.sleep(0.04)
        expected = cells(reference.capture())
        cursor = reference.cursor()
    with pane(prompt, **options) as actual:
        frames = []

        def completed():
            state = actual.state()
            capture = actual.capture()
            if state == "1 0":
                frames.append(capture)
            return frames and state == "0 1" and cells(capture) == expected

        actual.wait(completed)
        assert actual.cursor() == cursor, (name, "cursor changed", cursor, actual.cursor())
        floor = max(2, options.get("y", 7) - 3)
        settled = [cell for cell in expected if cell[0] >= floor]
        assert settled in [cells(frame) for frame in frames], (name, "native final frame differs", settled, cells(frames[-1]))
        assert any("38;2;" in frame and frame.count("█") > 20 for frame in frames), (name, "colored cuboids missing")
        actual.check_cleanup()
    print("PASS", name, "prompt cells, styles, cursor, colored cuboids, and hook cleanup")


def queued_input():
    with pane(mode="relative") as actual:
        actual.wait(lambda: actual.state() == "1 0" and "█" in actual.capture())
        marker = actual.home / "typed"
        actual.send("printf x >> " + shlex.quote(str(marker)))
        actual.wait(lambda: actual.state() == "0 1" and marker.exists())
        assert marker.read_text() == "x"
        actual.check_cleanup()
    print("PASS queued input executes exactly once")


def interrupt(action):
    with pane() as actual:
        actual.wait(lambda: actual.state() == "1 0" and "█" in actual.capture())
        if action == "ctrl-c":
            actual.tmux("send-keys", "-t", "test", "C-c")
        elif action == "resize":
            actual.tmux("resize-window", "-t", "test", "-x", "80", "-y", "24")
        else:
            pid = int(actual.tmux("display-message", "-p", "-t", "test", "#{pane_pid}"))
            processes = [line.split(maxsplit=2) for line in subprocess.run(["ps", "-eo", "pid=,ppid=,comm="], capture_output=True, text=True, check=True).stdout.splitlines()]
            descendants = {pid}
            for _ in range(5):
                descendants |= {int(p) for p, parent, _ in processes if int(parent) in descendants}
            targets = [int(p) for p, _, command in processes if int(p) in descendants and Path(command).name == "shimmer"]
            assert len(targets) == 1, targets
            os.kill(targets[0], signal.SIGTERM if action == "term" else signal.SIGHUP)
        actual.wait(lambda: actual.state() == "0 1" and "PS1$" in actual.capture())
        capture = actual.capture()
        assert "KEEP_BEFORE" in capture and "shimmer:" not in capture, capture
        actual.check_cleanup()
    print("PASS", action, "restores screen, cursor, and hooks")


def shimmer_and_missing_binary():
    with pane(effect="shimmer") as actual:
        actual.wait(lambda: actual.state() == "1 0")
        actual.wait(lambda: "48;5;" in actual.capture())
        actual.wait(lambda: actual.state() == "0 1" and "PS1$" in actual.capture())
        assert "KEEP_BEFORE" in actual.capture()
        actual.check_cleanup()
    with pane(prompt="STATUS:$? PS1$ ", binary="/does/not/exist", last="false") as actual:
        actual.wait(lambda: "STATUS:1 PS1$" in actual.capture())
        assert "binary not found" in actual.capture()
        actual.check_cleanup()
    print("PASS shimmer lifecycle and missing-binary status restoration")


def bounded_tmux_query():
    with tempfile.TemporaryDirectory(prefix="shimmer-slow-tmux-") as directory:
        command = Path(directory) / "tmux"
        command.write_text("#!/bin/sh\nexec sleep 10\n")
        command.chmod(0o755)
        setup = "PATH=" + shlex.quote(directory) + ":$PATH\nexport PATH\n"
        with pane(setup=setup) as actual:
            started = time.monotonic()
            actual.wait(lambda: actual.state() == "1 0" and "█" in actual.capture())
            assert time.monotonic() - started < 2, "tmux query exceeded its deadline"
            actual.wait(lambda: actual.state() == "0 1" and "PS1$" in actual.capture())
            actual.check_cleanup()
    print("PASS slow tmux query falls back within its deadline")


if __name__ == "__main__":
    assert BINARY.is_file(), BINARY
    landing("native midline", r"\[\e[1;32m\]PS1\[\e[0m\]:\[\e[38;2;255;120;40;48;5;17m\]path\[\e[0m\]$ ", x=16)
    landing("native bottom wrap", r"\[\e[38;5;45m\]username@host:verylongpath/for/wrapping\[\e[0m\]$ ", width=28, height=12, y=11)
    landing("native Unicode multiline", r"\[\e[35m\]界é\[\e[0m\]\nsecond ❯ ", width=40, height=12, y=11)
    landing("sparse hooks and status", "STATUS:$? PS1$ ", last="false")
    landing("relative midline", r"\[\e[1;32m\]PS1\[\e[0m\]$ ", mode="relative", x=16)
    landing("relative bottom wrap", "username@host:verylongpath/for/wrapping$ ", mode="relative", width=28, height=12, y=11)
    landing("stale tmux pane", "PS1$ ", mode="stale")
    queued_input()
    for action in ("ctrl-c", "term", "hup", "resize"):
        interrupt(action)
    shimmer_and_missing_binary()
    bounded_tmux_query()
