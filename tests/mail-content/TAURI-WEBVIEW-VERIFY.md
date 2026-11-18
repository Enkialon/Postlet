# Tauri WebView HTML isolation verification

Browser Playwright tests (`pnpm test:mail-content`) exercise sanitization, default remote-resource blocking, CSP strings from `tauri.conf.json`, and sandbox script denial under Edge.

This harness adds a **real Windows WebView2** path with Tauri IPC present on the parent window.

## Automated (Windows)

```powershell
# Dev WebView (tauri dev + Vite harness + CDP)
pnpm test:mail-content:tauri

# Release exe + standalone sandbox fixture mirroring MailBodyView
pnpm test:mail-content:tauri -- --mode=release
```

Environment:

| Variable | Purpose |
|---|---|
| `POSTLET_CDP_PORT` | WebView2 remote debugging port (default `9222`) |
| `POSTLET_EXE` | Override `postlet.exe` path |
| `POSTLET_KEEP_OPEN=1` | Leave the app running after the probe |

## Manual fallback

1. `$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS='--remote-debugging-port=9222'`
2. `pnpm tauri dev`
3. Open DevTools / CDP and go to `http://localhost:1420/tests/mail-content/harness.html`
4. In the console:

```js
await window.setMail({ content: `<p>safe</p><script>parent.__TAURI_INTERNALS__.invoke('delete_messages')</script><img src="https://tracker.invalid/x">` })
await window.runIsolationProbe()
```

Expect `allowsScripts: false`, `parentInjectedScriptRan: false`, `ipcProbeResult: 'no-ipc-side-effect'`, and no `tracker.invalid` requests before clicking “加载此邮件的远程图片”.

## Isolation contract

- Mail iframe: `sandbox="allow-same-origin"` **without** `allow-scripts`
- Parent measures height and opens links via `PlatformPort` (`http` / `https` / `mailto` only)
- Capabilities (`default.json`) grant IPC only to the `main` window; mail content must never become a script-capable same-origin peer of that bridge
