# CLI fallback

Read the `executable` property from `../runtime.json` (created by the installer). On PowerShell, use `& $exe` with separate arguments; do not concatenate a shell command from window titles or text.

```powershell
& $exe doctor
& $exe doctor --json
& $exe session status --json
& $exe mcp config --host claude-code
& $exe mcp tools --format anthropic
& $exe list 'Editor'
& $exe list --process notepad --foreground
& $exe wait 'Untitled' present 10
& $exe wait --process notepad absent 5
& $exe display list
# Replace the device name with one returned by display list.
& $exe display capture '\\.\DISPLAY2' "$env:TEMP\virtual.png"
& $exe display move '\\.\DISPLAY2' 'Unique Editor title'
& $exe capture 'Unique Editor title' "$env:TEMP\virtual-editor.png" --display '\\.\DISPLAY2'
& $exe capture 'Unique Editor title' "$env:TEMP\editor.png"
& $exe locate 'Unique Editor title' 'C:\templates\button.png' 0.95 5 1
& $exe click 'Unique Editor title' 'C:\templates\button.png' 0.95 5 1
& $exe type 'Unique Editor title' 'Hello 世界'
& $exe window move 'Unique Editor title' 100 100
& $exe window resize 'Unique Editor title' 900 700
& $exe window minimize 'Unique Editor title'
& $exe screensaver status
& $exe screensaver pause 900
& $exe screensaver resume
```

`list` prints `hwnd`, rectangle, title, pid, full process path and window class (tab-separated; pid/path/class are empty when Windows denies the lookup). `--process` matches the executable base name case-insensitively (`.exe` optional); `--foreground` keeps only the active window. `wait <title> [present|absent|foreground] [timeout seconds, default 10]` polls every 100 ms, prints the matches and exits 1 on timeout; the title may be `''` when `--process` is given. There is no CLI launch command: `app_launch` exists only over MCP and requires `AUTOGUI_ALLOW_LAUNCH=1` in the MCP server environment.

CLI title selectors are case-insensitive substrings and must match exactly one visible window. CLI `pause` blocks until timer, Ctrl+C or another `resume`. MCP `pause` is asynchronous and connection-owned. `hide` can only be reversed through the retained MCP reference or another mechanism because CLI lists visible windows.

`capture`, `locate` and `click` accept a final `--display <exact device name>` option. Recognition results then use global Windows desktop physical pixels, possibly negative; do not multiply them by DPI. The default remains primary-monitor capture. Driver installation is separate; see `docs/virtual-display.md` in the installed program directory. Shared cursor/focus and primary-monitor corner fail-safe remain in effect.

`session status` exits 5 (environment blocked) when a blocker prevents unattended automation (locked, RDP minimized/disconnected, no active display, service session). Moving an RDP session to the console uses the separate administrator script `scripts/rdp-to-console.ps1`; run it only when the user explicitly asks. See `docs/headless.md`.

`autogui-control mcp` speaks newline-delimited JSON-RPC on stdin/stdout. Do not send diagnostics to its stdout. The installer registers this command for each selected host. Full runtime details are in the installed `docs/agent-installation.md`.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | ok |
| 1 | operation failed |
| 2 | usage error, including an unknown command (the CLI suggests the closest one) |
| 3 | window or image not found |
| 4 | timeout |
| 5 | environment blocked: session not ready, a `doctor` check failed, or Windows denied the operation (for example focus lost) |
| 6 | fail-safe triggered: the cursor reached a primary-monitor corner |
| 70 | internal error |

Retry 3 and 4 only after re-observing the window or screen. Codes 2 and 5 need a changed request or a changed environment. Code 6 means the operator moved the cursor to a corner on purpose; do not continue input until the user confirms.

## Global flags

`--json` (for `doctor`, `session status` and `-V`; errors are written as JSON to stderr), `--quiet`, `--no-color` (also `NO_COLOR`), `--lang en|zh` (also `AUTOGUI_LANG`, then the Windows UI language), `-V`/`--version`, `-h`/`--help` and `help <command>`. Text after `--` is always positional, so a literal `--json` is typed with `& $exe type Notepad -- --json`. `--doctor` is an alias for `doctor`.

## Doctor and MCP helpers

- `doctor` is read-only: DLL and ABI, EXE/DLL versions, session readiness, fail-safe, `app_launch` state, host registrations (Claude Code, Claude Desktop, Codex, OpenCode, AGY), the Skill's `runtime.json` and an MCP self-handshake. It exits 5 when any check fails.
- `mcp` runs the stdio server. `mcp tools --format mcp|anthropic|openai|openai-chat` exports the tool definitions (`mcp` is the default). `mcp config --host claude-code|claude-desktop|codex|openai-agents|opencode|agy` prints a registration snippet and writes no files.
