#!/usr/bin/env python3
"""Record the real Bash hook in tmux and render its timed ANSI captures.

Development dependencies: Python 3.10+, Pillow, Bash 5.1+, and tmux.
"""

import argparse
from dataclasses import asdict, dataclass
import gzip
import hashlib
import json
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import tempfile
import time
import unicodedata

from PIL import Image, ImageDraw, ImageFont


WIDTH, HEIGHT = 96, 28
BACKGROUND, FOREGROUND = "#071018", "#d4e2eb"
COLORS = (
    "#000000", "#cd0000", "#00cd00", "#cdcd00", "#0000ee", "#cd00cd", "#00cdcd", "#e5e5e5",
    "#7f7f7f", "#ff0000", "#00ff00", "#ffff00", "#5c5cff", "#ff00ff", "#00ffff", "#ffffff",
)
SGR = re.compile(r"\x1b\[([0-9;:]*)m")


@dataclass
class Frame:
    elapsed_ms: float
    ansi: str
    alternate: bool
    cursor_visible: bool
    cursor_x: int
    cursor_y: int


class Pane:
    def __init__(self, socket):
        self.socket = socket

    def tmux(self, *args):
        return subprocess.run(
            ["tmux", "-S", str(self.socket), *args],
            capture_output=True, text=True, check=True,
        ).stdout

    def capture(self, started):
        before = time.monotonic()
        output = self.tmux(
            "capture-pane", "-p", "-e", "-N", "-t", "demo", ";",
            "display-message", "-p", "-t", "demo",
            "#{alternate_on} #{cursor_flag} #{cursor_x} #{cursor_y}",
        )
        ansi, state = output.rstrip("\n").rsplit("\n", 1)
        alternate, visible, x, y = map(int, state.split())
        return Frame(
            ((before + time.monotonic()) / 2 - started) * 1000,
            ansi, bool(alternate), bool(visible), x, y,
        )


def record(binary, hook, effect):
    with tempfile.TemporaryDirectory(prefix="shimmer-demo-") as directory:
        home = Path(directory)
        (home / "shimmer").mkdir()
        rc = home / "rc.bash"
        rc.write_text(
            "HISTFILE=/dev/null\n"
            "PS1=" + shlex.quote(
                r"\[\e[1;32m\]\u@terminal\[\e[0m\]:"
                r"\[\e[1;38;5;75m\]\w\[\e[0m\] \[\e[38;5;45m\]❯\[\e[0m\] "
            ) + "\n"
            "cd ~/shimmer\n"
            "printf '\\033[2J\\033[H'\n"
            "printf 'Bash %s\\n' \"${BASH_VERSION%%(*}\"\n"
            "\"$SHELL_SHIMMER_BIN\" --version\n"
            "printf '\\nChoose an effect in ~/.config/shell/shimmer.conf\\n'\n"
            "PROMPT_COMMAND='printf \"\\033[8;1H\\033[K\"'\n"
        )
        pane = Pane(home / "tmux.sock")
        try:
            pane.tmux(
                "-f", "/dev/null", "new-session", "-d", "-s", "demo",
                "-x", str(WIDTH), "-y", str(HEIGHT),
                "-e", "HOME=" + str(home), "-e", "TERM=tmux-256color",
                "-e", "SHELL_SHIMMER_BIN=" + str(binary),
                "-e", "SHELL_SHIMMER_EFFECT=" + effect,
                "-e", "DEMO_HOOK=" + str(hook),
                "bash --noprofile --rcfile " + shlex.quote(str(rc)) + " -i",
            )
            pane.tmux("set-option", "-g", "status", "off")
            pane.tmux("resize-window", "-t", "demo", "-x", str(WIDTH), "-y", str(HEIGHT))
            deadline = time.monotonic() + 5
            while True:
                initial = pane.capture(time.monotonic())
                if "❯" in initial.ansi and initial.cursor_y == 7:
                    break
                if time.monotonic() > deadline:
                    raise RuntimeError("Bash did not reach its initial prompt")
                time.sleep(0.02)
            started = time.monotonic()
            initial.elapsed_ms = 0
            frames = [initial]
            time.sleep(0.5)
            pane.tmux("send-keys", "-t", "demo", "-l", '. "$DEMO_HOOK"')
            pane.tmux("send-keys", "-t", "demo", "Enter")
            seen_effect = False
            next_sample = time.monotonic()
            while time.monotonic() - started < 5:
                frame = pane.capture(started)
                frames.append(frame)
                seen_effect |= frame.alternate
                if seen_effect and not frame.alternate and frame.cursor_visible:
                    if (frame.ansi, frame.cursor_x, frame.cursor_y) != (
                        initial.ansi, initial.cursor_x, initial.cursor_y
                    ):
                        raise RuntimeError("The restored prompt differs from the initial prompt")
                    break
                next_sample += 1 / 30
                time.sleep(max(0, next_sample - time.monotonic()))
            else:
                raise RuntimeError("The effect did not enter and leave the alternate screen")
            time.sleep(0.8)
            frames.append(pane.capture(started))
        finally:
            subprocess.run(["tmux", "-S", str(pane.socket), "kill-server"], capture_output=True)
        active = [frame for frame in frames if frame.alternate]
        if effect == "assemble" and not any("38;2;" in frame.ansi and "█" in frame.ansi for frame in active):
            raise RuntimeError("The capture missed the colored cuboids")
        if effect == "shimmer" and not any("48;5;" in frame.ansi for frame in active):
            raise RuntimeError("The capture missed the colored sweep")
        return frames


def color(value, default):
    if value is None:
        return default
    if isinstance(value, tuple):
        return value
    if value < 16:
        return COLORS[value]
    if value >= 232:
        return (8 + 10 * (value - 232),) * 3
    value -= 16
    scale = (0, 95, 135, 175, 215, 255)
    return scale[value // 36], scale[value // 6 % 6], scale[value % 6]


def cells(capture):
    foreground = background = None
    bold = False
    for row, line in enumerate(capture.splitlines()):
        column = index = 0
        while index < len(line):
            match = SGR.match(line, index)
            if match:
                codes = [int(code or "0") for code in match[1].replace(":", ";").split(";")]
                k = 0
                while k < len(codes):
                    code = codes[k]
                    if code == 0:
                        foreground = background = None
                        bold = False
                    elif code in (1, 22):
                        bold = code == 1
                    elif code == 39:
                        foreground = None
                    elif code == 49:
                        background = None
                    elif 30 <= code <= 37 or 90 <= code <= 97:
                        foreground = code - (30 if code < 90 else 82)
                    elif 40 <= code <= 47 or 100 <= code <= 107:
                        background = code - (40 if code < 100 else 92)
                    elif code in (38, 48):
                        if codes[k + 1] == 5:
                            value = codes[k + 2]
                            k += 2
                        elif codes[k + 1] == 2:
                            value = tuple(codes[k + 2:k + 5])
                            k += 4
                        else:
                            raise ValueError("Unsupported ANSI color")
                        if code == 38:
                            foreground = value
                        else:
                            background = value
                    k += 1
                index = match.end()
                continue
            glyph = line[index]
            index += 1
            while index < len(line) and unicodedata.category(line[index]) in ("Mn", "Me"):
                glyph += line[index]
                index += 1
            span = 2 if unicodedata.east_asian_width(glyph[0]) in ("W", "F") else 1
            yield row, column, glyph, span, foreground, background, bold
            column += span


def render(frame, font, bold_font):
    cell_width = round(font.getlength("M"))
    cell_height = sum(font.getmetrics())
    image = Image.new("RGB", (WIDTH * cell_width, HEIGHT * cell_height), BACKGROUND)
    draw = ImageDraw.Draw(image)
    for row, column, glyph, span, foreground, background, bold in cells(frame.ansi):
        x, y = column * cell_width, row * cell_height
        rectangle = (x, y, x + span * cell_width - 1, y + cell_height - 1)
        draw.rectangle(rectangle, fill=color(background, BACKGROUND))
        if glyph == "█":
            draw.rectangle(rectangle, fill=color(foreground, FOREGROUND))
        elif not glyph.isspace():
            draw.text((x, y), glyph, font=bold_font if bold else font, fill=color(foreground, FOREGROUND))
    if frame.cursor_visible:
        x, y = frame.cursor_x * cell_width, frame.cursor_y * cell_height
        draw.rectangle((x, y, x + cell_width - 1, y + cell_height - 1), fill=FOREGROUND)
    return image


def font_path(requested):
    if requested:
        return requested.resolve()
    for candidate in (
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
        "/usr/share/fonts/truetype/liberation2/LiberationMono-Regular.ttf",
        "/System/Library/Fonts/Menlo.ttc",
    ):
        if Path(candidate).is_file():
            return Path(candidate)
    raise SystemExit("No monospace font found. Pass --font PATH and optionally --bold-font PATH.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--output", type=Path, default=Path("docs/demos"))
    parser.add_argument("--font", type=Path)
    parser.add_argument("--bold-font", type=Path)
    args = parser.parse_args()
    binary = args.binary.resolve()
    if not binary.is_file():
        parser.error("binary must be an existing executable")
    for command in ("bash", "tmux"):
        if not shutil.which(command):
            parser.error(command + " is required")
    regular = font_path(args.font)
    bold = args.bold_font or regular.with_name(regular.stem + "-Bold" + regular.suffix)
    if not bold.is_file():
        bold = regular
    font = ImageFont.truetype(str(regular), 15)
    bold_font = ImageFont.truetype(str(bold), 15)
    args.output.mkdir(parents=True, exist_ok=True)
    hook = Path(__file__).resolve().parent / "shell-shimmer.bash"
    manifest = {
        "binary_version": subprocess.check_output([str(binary), "--version"], text=True).strip(),
        "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "hook_sha256": hashlib.sha256(hook.read_bytes()).hexdigest(),
        "terminal_columns": WIDTH, "terminal_rows": HEIGHT, "target_fps": 30,
        "font": regular.name, "font_size": 15,
        "tmux_version": subprocess.check_output(["tmux", "-V"], text=True).strip(),
        "effects": {},
    }
    for effect in ("assemble", "shimmer"):
        frames = record(binary, hook, effect)
        if any(frame.cursor_visible for frame in frames if frame.alternate):
            raise RuntimeError("The effect did not hide the cursor")
        if effect == "assemble":
            expected = [cell for cell in cells(frames[0].ansi) if cell[0] >= 7 and not cell[2].isspace()]
            landing = [frame for frame in frames if frame.alternate][-1]
            actual = [cell for cell in cells(landing.ansi) if not cell[2].isspace()]
            if actual != expected:
                raise RuntimeError("The native final frame differs from the real Bash prompt")
        images = [render(frame, font, bold_font) for frame in frames]
        ticks = [round(frame.elapsed_ms / 10) for frame in frames]
        durations = [max(10, (b - a) * 10) for a, b in zip(ticks, ticks[1:])]
        durations.append(10)
        images[0].save(
            args.output / (effect + ".gif"), save_all=True, append_images=images[1:],
            duration=durations, loop=0, optimize=True, disposal=1,
        )
        active = [i for i, frame in enumerate(frames) if frame.alternate]
        still = active[len(active) // (3 if effect == "assemble" else 2)]
        images[still].save(args.output / (effect + ".png"))
        with gzip.open(args.output / (effect + ".frames.json.gz"), "wt", encoding="utf-8") as output:
            json.dump([asdict(frame) for frame in frames], output, ensure_ascii=False)
        with Image.open(args.output / (effect + ".gif")) as gif:
            total_ms = 0
            for index in range(gif.n_frames):
                gif.seek(index)
                total_ms += gif.info["duration"]
            if gif.n_frames < 5 or total_ms < 1500:
                raise RuntimeError("The GIF lost its animation or timing")
        manifest["effects"][effect] = {
            "captured_frames": len(frames), "gif_frames": gif.n_frames,
            "gif_duration_ms": total_ms, "still_elapsed_ms": round(frames[still].elapsed_ms),
            "alternate_first_ms": round(frames[active[0]].elapsed_ms),
            "alternate_last_ms": round(frames[active[-1]].elapsed_ms),
            "restored_cursor": [frames[-1].cursor_x, frames[-1].cursor_y],
            "initial_and_restored_screen_equal": True,
            "native_landing_matches_prompt": True if effect == "assemble" else None,
        }
        print(effect, manifest["effects"][effect])
    (args.output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")


if __name__ == "__main__":
    main()
