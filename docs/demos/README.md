# Record the terminal demos

These GIFs record `shimmer 0.1.3` running through the committed Bash hook in an interactive Bash shell. The [manifest](manifest.json) records the binary's SHA-256 hash, hook hash, terminal geometry, font, frame counts, and timing.

Use Bash 5.1 or newer, which Shimmer also needs at runtime. Recording additionally needs Python 3.10 or newer, Pillow, and tmux as development tools. Installed releases need neither Python nor Pillow. They work inside or outside tmux.

From the repository root, run the recorder with a release binary.

```sh
python3 -m venv .audit/demo-venv
.audit/demo-venv/bin/pip install Pillow
.audit/demo-venv/bin/python scripts/record-demos.py target/release/shimmer
```

To record a downloaded release, replace `target/release/shimmer` with its executable path. Use `--output DIRECTORY` to save another recording. Use `--font PATH` and `--bold-font PATH` to select monospace TrueType fonts. The default font search supports DejaVu Sans Mono, Liberation Mono, and Menlo.

The recorder creates an isolated home directory and tmux socket. It runs Bash without the user's profile or rc file, disables history writes, and sources `scripts/shell-glimmer.bash` with `SHELL_GLIMMER_BIN` pointing to the supplied executable. It removes the temporary terminal and home directory after each effect.

The terminal has 96 columns and 28 rows. Its real, colored `PS1` uses the current username, a fixture host label of `terminal`, `~/shimmer`, and a prompt arrow. An existing `PROMPT_COMMAND` places that prompt at row 8. The recorder starts the effect by sourcing the hook in that shell. Assemble lands at the same prompt coordinates. The native binary restores the prior screen, then Bash displays the prompt.

The recorder samples `tmux capture-pane -p -e -N` at about 30 frames per second. Each sample includes its elapsed time, alternate screen state, cursor visibility, and cursor position. Pillow renders the captured ANSI foreground colors, background colors, bold text, full block cells, and visible cursor into the full viewport. Whitespace retains its background color.

GIF delays follow the elapsed capture times, rounded cumulatively to GIF's 10 ms units. The recordings include about 0.5 seconds before each effect and 0.8 seconds after it. The native animation runs at its original speed. Assemble takes about 1.2 seconds, and shimmer takes about 0.75 seconds. GIF encoding merges identical frames. No frames are generated to simulate either effect.

The recorder checks that both effects enter and leave the alternate screen, hide the cursor during the effect, and restore the identical ANSI screen and cursor. For assemble, it also compares the native final frame's prompt glyphs, styles, and coordinates with the real initial Bash prompt. It checks each saved GIF for multiple frames and a meaningful duration.

Each effect has an animated GIF, a PNG still taken during the effect, and a compressed JSON record of the original timed captures.

| Effect | Animation | Still | Raw captures |
| --- | --- | --- | --- |
| Assemble | [assemble.gif](assemble.gif) | [assemble.png](assemble.png) | [assemble.frames.json.gz](assemble.frames.json.gz) |
| Shimmer | [shimmer.gif](shimmer.gif) | [shimmer.png](shimmer.png) | [shimmer.frames.json.gz](shimmer.frames.json.gz) |

Assemble uses a fresh random arrangement on each run. Repeating the command proves the same terminal behavior without reproducing identical pixels.
