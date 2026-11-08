# Add a shell glimmer

Show a short animation before the next interactive Bash prompt. Choose the cyan, lime, and white `shimmer` sweep or `assemble`, which flies colorful 3D prompt fragments into the prompt's native position. The previous terminal screen returns, then Bash prints the prompt once.

Use Bash 5.1 or newer and a terminal with ANSI colors and an alternate screen. The native `shimmer` binary needs neither Rust nor Python after installation. Older Bash versions skip the animation. macOS ships Bash 3.2, so install a current Bash and use it as your interactive shell. Both effects work inside and outside tmux.

## Install

Install a [release bundle or source build](../README.md). From the repository root, `make install` builds the optimized binary and runs the installer. The installer preserves your effect configuration and installs these files:

- `~/.local/bin/shimmer` is the renderer.
- `~/.config/shell/glimmer.bash` is the Bash hook.
- `~/.config/shell/glimmer.conf` selects the effect.

The installer places this guarded source line once at the end of `~/.bashrc`, after all `PS1` and `PROMPT_COMMAND` setup, including prompt hooks such as direnv:

```bash
[[ -r ~/.config/shell/glimmer.bash ]] && source ~/.config/shell/glimmer.bash
```

Keep that line after any prompt hooks you add later. The hook runs the binary directly and never compiles code at shell startup. For development, set `SHELL_GLIMMER_BIN` to the full path of a different `shimmer` executable.

Open a new interactive Bash shell to see the animation. The script skips noninteractive shells, redirected terminals, and `TERM=dumb`.

## Select an effect

Set one value in `~/.config/shell/glimmer.conf`:

```ini
effect=assemble
```

Use `shimmer` for the original sweep or `off` to disable both effects. The example configuration selects `shimmer`.

The configuration is data and is never sourced. Spaces around `effect`, `=`, and the value are allowed. Blank lines, comment lines, and invalid lines are ignored. The last valid `effect` wins. A missing file or a file without a valid value selects `shimmer`.

## Preview

In an interactive Bash terminal, run either command:

```bash
SHELL_GLIMMER_EFFECT=assemble source ~/.config/shell/glimmer.bash
SHELL_GLIMMER_EFFECT=shimmer source ~/.config/shell/glimmer.bash
```

The effect runs at the next prompt, after existing prompt hooks. The temporary selector is captured when you source the script. A valid `SHELL_GLIMMER_EFFECT` overrides the configuration. An invalid value leaves the configured effect selected.

Sourcing again before that prompt replaces the pending effect. Selecting `off` cancels it. To disable animations unconditionally, set this before the source line in `~/.bashrc`:

```bash
export SHELL_GLIMMER=0
```

This setting overrides both selectors and skips prompt expansion. Remove it to enable the selected effect again.

## Prompt behavior

The script appends one temporary `PROMPT_COMMAND` callback. Bash runs the existing hooks normally before that callback. The callback restores the original unset, scalar, or indexed-array hook, including its export attribute and sparse array indices, and returns the incoming command status. If another hook changes `PROMPT_COMMAND`, the callback preserves that change and removes only its own entry. Read-only, associative-array, and nameref hooks skip the animation.

For `assemble`, the callback expands `${PS1@P}` after those hooks. Hook output, directory changes, and changes to `PS1` appear in the snapshot. The script never evaluates hook strings or configuration itself. If the renderer is missing or fails, the hook reports the problem and returns the incoming command status. After the child renderer exits, the callback removes its private functions and state. No persistent traps remain.

If an existing hook makes `PROMPT_COMMAND` read-only while the animation is pending, Bash prevents removal of the temporary entry. That entry and a function that only returns the incoming status remain until the shell exits. The animation still runs once, and all private pending state is cleared. Sourcing again preserves that inert function.

Bash expands `PS1` again when it displays the real prompt. Command substitutions and other changing prompt values can therefore run twice and produce different text. This snapshot limitation also applies when geometry checks skip the animation. Use a stable prompt value if both frames must match exactly.

## Native layout and limits

When the current terminal matches a tmux pane, `assemble` measures its cursor and dimensions without reading terminal input. The fragments land at that measured position, including a nonzero starting column. The last animation frame uses the prompt's original text, spacing, and ANSI styles, including default colors, 16-color, 256-color, RGB, bold, and resets.

Fragment size and flight depth adapt to terminal area, with bounds for very small and large terminals. The final prompt keeps its native size and position.

Long lines wrap at the terminal edge. Tabs use absolute eight-column stops, and carriage returns and newlines update the native layout. Common wide characters and combining marks are supported. For a matching tmux pane, if the prompt needs to scroll, the renderer scrolls the main screen by that amount before entering the alternate screen. Bash then displays the prompt at the same shifted origin. Existing output remains in scrollback.

Outside a matching tmux pane, `assemble` saves the native cursor on the alternate screen and draws fragments with movement relative to that cursor. Flight geometry uses a virtual anchor with room above and below it; the terminal may clip moving fragments near an edge. The final frames restore the saved cursor and print the sanitized prompt sequentially, so the terminal handles its actual starting column, tabs, wrapping, and scrolling. This path never applies scrolling calculated from virtual geometry to the main screen. Leaving the alternate screen restores the prompt origin, and Bash prints the real prompt there once.

Readline markers, terminal title commands, hyperlinks, cursor commands, and other non-SGR controls are removed from the snapshot. Cursor-moving prompts, complex emoji sequences, script shaping, and terminal-specific ambiguous character widths can differ from the animation. Prompts exceeding the supported layout can skip assembly. An unavailable or mismatched tmux pane uses relative drawing. The drawing area is capped at 500 columns and 200 rows; larger terminals also use relative drawing.

Some terminals discard pending autowrap when switching screens. A hook that leaves the cursor waiting to wrap after the last column can therefore change where the first prompt character lands. Ordinary prompt origins and wrapping within the prompt are supported; this pre-existing cursor state is outside the supported geometry.

A resize or interrupt ends the animation and restores the screen and cursor. The renderer never reads keyboard input. `shimmer` retains its original `0.10`-second clear, `25` frames over `0.50` seconds, and `0.15`-second hold. `assemble` starts drawing after the clear, with `33` frames over `1.05` seconds and a `0.15`-second hold.
