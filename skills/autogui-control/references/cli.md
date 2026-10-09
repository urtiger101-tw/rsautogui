# CLI fallback

Read the `executable` property from `../runtime.json` (created by the installer). On PowerShell, use `& $exe` with separate arguments; do not concatenate a shell command from window titles or text.

```powershell
& $exe session status --json
& $exe list 'Editor'
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

CLI title selectors are case-insensitive substrings and must match exactly one visible window. CLI `pause` blocks until timer, Ctrl+C or another `resume`. MCP `pause` is asynchronous and connection-owned. `hide` can only be reversed through the retained MCP reference or another mechanism because CLI lists visible windows.

`capture`, `locate` and `click` accept a final `--display <exact device name>` option. Recognition results then use global Windows desktop physical pixels, possibly negative; do not multiply them by DPI. The default remains primary-monitor capture. Driver installation is separate; see `docs/virtual-display.md` in the installed program directory. Shared cursor/focus and primary-monitor corner fail-safe remain in effect.

`session status` exits 1 when a blocker prevents unattended automation (locked, RDP minimized/disconnected, no active display, service session). Moving an RDP session to the console uses the separate administrator script `scripts/rdp-to-console.ps1`; run it only when the user explicitly asks. See `docs/headless.md`.

`autogui-control mcp` speaks newline-delimited JSON-RPC on stdin/stdout. Do not send diagnostics to its stdout. The installer registers this command for each selected host. Full runtime details are in the installed `docs/agent-installation.md`.
