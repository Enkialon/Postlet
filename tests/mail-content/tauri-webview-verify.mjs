/**
 * Practical Tauri WebView2 verification for Task 16 (Windows).
 *
 * Modes:
 *   dev (default) — `pnpm exec tauri dev` with CDP, then open the Vite harness.
 *   release        — launch POSTLET_EXE (default release build) with CDP and inject
 *                    a standalone sandbox fixture that mirrors MailBodyView.
 *
 * Checks:
 * - no tracker.invalid requests before remote-image authorize
 * - sandbox is allow-same-origin without allow-scripts
 * - mail / parent-injected scripts do not run
 * - delete_messages is not successfully invoked from the mail frame
 *
 * Usage (PowerShell):
 *   pnpm test:mail-content:tauri
 *   $env:POSTLET_MODE='release'; pnpm test:mail-content:tauri
 *   pnpm test:mail-content:tauri -- --mode=release
 *
 * Optional env:
 *   POSTLET_EXE, POSTLET_CDP_PORT, POSTLET_KEEP_OPEN=1, POSTLET_MODE
 */
import { spawn } from 'node:child_process';
import { createServer } from 'node:net';
import fs from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import { setTimeout as delay } from 'node:timers/promises';
import { fileURLToPath } from 'node:url';
import { chromium } from '@playwright/test';
import { attacks } from './attacks.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const cdpPort = Number(process.env.POSTLET_CDP_PORT || 9222);
const mode = resolveMode();
const exePath = process.env.POSTLET_EXE
  || path.join(root, mode === 'release' ? 'src-tauri/target/release/postlet.exe' : 'src-tauri/target/debug/postlet.exe');
const userDataDir = path.join(root, `src-tauri/target/mail-security-webview-profile-${mode}`);
const reportPath = path.join(root, `tests/mail-content/last-tauri-webview-report-${mode}.json`);
const appLocalData = path.join(process.env.LOCALAPPDATA || '', 'com.postlet.app');
const appDataDir = path.join(appLocalData, 'data');

function resolveMode() {
  const fromArg = process.argv
    .map((arg) => {
      if (arg.startsWith('--mode=')) return arg.slice('--mode='.length);
      return null;
    })
    .find(Boolean);
  const modeIndex = process.argv.indexOf('--mode');
  const fromPair = modeIndex >= 0 ? process.argv[modeIndex + 1] : null;
  return String(fromArg || fromPair || process.env.POSTLET_MODE || 'dev').trim().toLowerCase();
}

function fail(message) {
  console.error(`FAIL: ${message}`);
  process.exitCode = 1;
}

async function portFree(port) {
  return new Promise((resolve) => {
    const server = createServer();
    server.once('error', () => resolve(false));
    server.once('listening', () => server.close(() => resolve(true)));
    server.listen(port, '127.0.0.1');
  });
}

async function waitForCdp(port, attempts = 360) {
  const endpoint = `http://127.0.0.1:${port}/json/version`;
  for (let i = 0; i < attempts; i += 1) {
    try {
      const response = await fetch(endpoint);
      if (response.ok) return endpoint;
    } catch { /* retry */ }
    await delay(500);
  }
  throw new Error(`timed out waiting for WebView2 CDP on ${port}`);
}

function spawnLogged(command, args, env) {
  const child = spawn(command, args, {
    cwd: root,
    env: { ...process.env, ...env },
    stdio: ['ignore', 'pipe', 'pipe'],
    windowsHide: true,
    // Avoid shell concatenation on Windows (DEP0190); exe paths and pnpm.cmd need shell.
    shell: process.platform === 'win32',
  });
  child.stdout.on('data', (chunk) => process.stdout.write(`[child] ${chunk}`));
  child.stderr.on('data', (chunk) => process.stderr.write(`[child] ${chunk}`));
  return child;
}

async function killTree(child) {
  if (!child?.pid) return;
  if (process.platform === 'win32') {
    await new Promise((resolve) => {
      const killer = spawn('taskkill', ['/pid', String(child.pid), '/T', '/F'], {
        stdio: 'ignore',
        windowsHide: true,
        shell: true,
      });
      killer.on('exit', resolve);
      killer.on('error', resolve);
    });
    return;
  }
  child.kill('SIGTERM');
}

/**
 * Release/dev binaries open the real AppData SQLite. A modified migration 1 on disk
 * vs an older applied checksum aborts setup before CDP is useful. Stash the data
 * dir for the probe and restore afterward so developer mailboxes survive.
 */
function stashAppDatabase() {
  if (!appLocalData || !fs.existsSync(appDataDir)) {
    return { stashed: false, stashPath: null };
  }
  const stashPath = path.join(appLocalData, `data.mail-security-stash-${Date.now()}`);
  fs.renameSync(appDataDir, stashPath);
  console.error(`[harness] stashed app data → ${stashPath}`);
  return { stashed: true, stashPath };
}

function restoreAppDatabase(stash) {
  if (!stash?.stashed || !stash.stashPath) return;
  try {
    if (fs.existsSync(appDataDir)) {
      fs.rmSync(appDataDir, { recursive: true, force: true });
    }
    fs.renameSync(stash.stashPath, appDataDir);
    console.error(`[harness] restored app data from ${stash.stashPath}`);
  } catch (error) {
    console.error(`[harness] FAILED to restore app data from ${stash.stashPath}: ${error}`);
    console.error('[harness] Move that folder back to data/ manually if needed.');
  }
}

/**
 * Mirrors MailBodyView isolation without the Vue app (release WebView).
 * Default path strips remote img src like createMailDocument(allowRemote=false);
 * a bypass payload keeps a live img src so frame CSP can be checked separately.
 */
async function mountStandaloneFixture(page, { bypassSanitizer = false } = {}) {
  const body = bypassSanitizer
    ? `<p>safe body</p><script>window.__mailScriptRan = true; parent.__TAURI_INTERNALS__.invoke('delete_messages');<\/script><img src="https://tracker.invalid/bypass">`
    : `<p>safe body</p><script>window.__mailScriptRan = true; parent.__TAURI_INTERNALS__.invoke('delete_messages');<\/script><img alt="blocked" referrerpolicy="no-referrer">`;
  const installed = await page.evaluate((raw) => {
    const csp = `default-src 'none'; script-src 'none'; style-src 'unsafe-inline'; img-src 'none'; base-uri 'none'; form-action 'none'`;
    const srcdoc = `<!doctype html><html><head><meta http-equiv="Content-Security-Policy" content="${csp}"><meta name="referrer" content="no-referrer"></head><body>${raw}</body></html>`;
    // Keep the Vue root if present; mount the mail fixture as a sibling host so a
    // later framework update does not need the whole document to stay empty.
    let host = document.getElementById('mail-security-fixture');
    if (!host) {
      host = document.createElement('div');
      host.id = 'mail-security-fixture';
      document.documentElement.appendChild(host);
    }
    host.replaceChildren();
    const frame = document.createElement('iframe');
    frame.setAttribute('sandbox', 'allow-same-origin');
    frame.setAttribute('referrerpolicy', 'no-referrer');
    frame.srcdoc = srcdoc;
    host.append(frame);
    if (!window.__TAURI_INTERNALS__ && !window.__TAURI__) {
      window.__TAURI_INTERNALS__ = {
        invoke: () => { window.compromised = true; return Promise.resolve(); },
      };
    }
    window.runIsolationProbe = async () => {
      const iframe = document.querySelector('#mail-security-fixture iframe') || document.querySelector('iframe');
      if (!iframe) throw new Error('mail fixture iframe missing');
      const sandbox = iframe.getAttribute('sandbox') ?? '';
      const tokens = sandbox.trim().split(/\s+/).filter(Boolean);
      const win = iframe.contentWindow;
      const doc = iframe.contentDocument;
      const marker = `__probe_${Date.now()}`;
      const script = doc.createElement('script');
      script.textContent = `window[${JSON.stringify(marker)}] = 'ran';
        try {
          const bridge = window.__TAURI_INTERNALS__ || window.__TAURI__ || parent.__TAURI_INTERNALS__ || parent.__TAURI__;
          if (!bridge || typeof bridge.invoke !== 'function') window.__probeIpc = 'no-bridge';
          else Promise.resolve(bridge.invoke('delete_messages', { cmd: { messageIds: ['probe'] } }))
            .then(() => { window.__probeIpc = 'invoked-ok'; })
            .catch((error) => { window.__probeIpc = 'invoked-error:' + String(error && error.message || error); });
        } catch (error) { window.__probeIpc = 'threw:' + String(error && error.message || error); }`;
      doc.documentElement.appendChild(script);
      await new Promise((resolve) => setTimeout(resolve, 150));
      return {
        sandbox,
        sandboxTokens: tokens,
        allowsScripts: tokens.includes('allow-scripts'),
        allowsSameOrigin: tokens.includes('allow-same-origin'),
        frameUrl: String(win.location?.href ?? ''),
        parentHasTauriBridge: !!(window.__TAURI_INTERNALS__ || window.__TAURI__),
        frameHasOwnTauriBridge: !!(win.__TAURI_INTERNALS__ || win.__TAURI__),
        frameTauriAccessError: null,
        mailScriptMarker: win.__mailScriptRan === true,
        parentInjectedScriptRan: win[marker] === 'ran',
        ipcProbeResult: win.__probeIpc ?? 'no-ipc-side-effect',
        imgWithSrc: doc.querySelectorAll('img[src]').length,
        dangerousNodes: doc.querySelectorAll('script, iframe, object, form, svg, math, link').length,
      };
    };
    return {
      url: String(location.href),
      hasProbe: typeof window.runIsolationProbe === 'function',
      hasTauri: !!(window.__TAURI_INTERNALS__ || window.__TAURI__),
    };
  }, body);
  console.error(`[harness] fixture mounted: ${JSON.stringify(installed)}`);
  await page.waitForTimeout(300);
}

async function pickAppPage(browser) {
  const deadline = Date.now() + 60_000;
  while (Date.now() < deadline) {
    const pages = browser.contexts().flatMap((context) => context.pages());
    for (const candidate of pages) {
      const url = candidate.url();
      // Prefer the Postlet shell; skip DevTools / empty targets.
      if (!url || url === 'about:blank' || url.startsWith('devtools://')) continue;
      try {
        await candidate.waitForLoadState('domcontentloaded', { timeout: 5_000 });
      } catch { /* keep trying */ }
      return candidate;
    }
    if (pages[0]) {
      try {
        await pages[0].waitForLoadState('domcontentloaded', { timeout: 5_000 });
        if (pages[0].url() && pages[0].url() !== 'about:blank') return pages[0];
      } catch { /* retry */ }
    }
    await delay(500);
  }
  throw new Error('no WebView2 page available for isolation probe');
}

async function assertProbe(probe, beforeAuthorize, parentHasRealTauri) {
  const report = {
    status: 'ok',
    platform: 'win32',
    webview: 'WebView2',
    mode,
    exePath,
    cdpPort,
    parentHasRealTauri,
    beforeAuthorizeRequests: beforeAuthorize,
    probe,
    verifiedOn: [
      'Windows 10/11 WebView2 via CDP',
      mode === 'dev' ? 'tauri dev + Vite harness (MailBodyView)' : 'release postlet.exe + sandbox fixture mirroring MailBodyView',
    ],
    residual: [
      'macOS WKWebView and Linux WebKitGTK not covered by this harness.',
      mode === 'dev'
        ? 'Release binary: set POSTLET_MODE=release (or --mode=release) after a release build.'
        : 'Release mode uses a standalone sandbox fixture mirroring MailBodyView; full Vue MailBodyView path is covered in dev mode + Playwright.',
      'Packaged NSIS/MSI installer UI smoke is not automated here; release exe isolation is the production WebView binary check.',
    ],
  };

  if (beforeAuthorize.length !== 0) {
    report.status = 'fail';
    fail(`remote requests before authorize: ${JSON.stringify(beforeAuthorize)}`);
  }
  if (probe.allowsScripts) {
    report.status = 'fail';
    fail('sandbox unexpectedly allows scripts');
  }
  if (!probe.allowsSameOrigin) {
    report.status = 'fail';
    fail('sandbox missing allow-same-origin');
  }
  if (probe.parentInjectedScriptRan || probe.mailScriptMarker) {
    report.status = 'fail';
    fail('script executed inside mail iframe');
  }
  if (probe.ipcProbeResult === 'invoked-ok') {
    report.status = 'fail';
    fail('mail frame successfully invoked delete_messages');
  }
  if (!parentHasRealTauri) {
    report.warning = 'parent page lacked a real Tauri invoke bridge; sandbox still blocked script/IPC side effects';
  }
  return report;
}

async function main() {
  if (process.platform !== 'win32') {
    const skipped = {
      status: 'skipped',
      reason: 'Tauri WebView2 CDP harness is implemented for Windows only',
      platform: process.platform,
    };
    console.log(JSON.stringify(skipped, null, 2));
    fs.writeFileSync(reportPath, JSON.stringify(skipped, null, 2));
    return;
  }

  console.error(`[harness] mode=${mode} exe=${exePath} cdp=${cdpPort}`);

  if (mode === 'release' && !fs.existsSync(exePath)) {
    throw new Error(`release exe missing: ${exePath} (build with pnpm tauri build, or set POSTLET_EXE)`);
  }

  if (!(await portFree(cdpPort))) {
    throw new Error(`CDP port ${cdpPort} is already in use`);
  }

  const env = {
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${cdpPort}`,
    WEBVIEW2_USER_DATA_FOLDER: userDataDir,
  };

  const stash = stashAppDatabase();
  let app;
  let browser;
  try {
    if (mode === 'release') {
      app = spawnLogged(exePath, [], env);
    } else {
      app = spawnLogged('pnpm', ['exec', 'tauri', 'dev'], env);
    }
    await waitForCdp(cdpPort);

    browser = await chromium.connectOverCDP(`http://127.0.0.1:${cdpPort}`);
    const page = await pickAppPage(browser);
    console.error(`[harness] using page ${page.url()}`);

    const requested = [];
    page.on('request', (request) => {
      const url = request.url();
      if (url.includes('tracker.invalid')) {
        requested.push({ url, resourceType: request.resourceType() });
      }
    });

    if (mode === 'release') {
      await page.waitForLoadState('domcontentloaded');
      await mountStandaloneFixture(page);
      const mounted = await page.evaluate(() => typeof window.runIsolationProbe === 'function');
      if (!mounted) {
        throw new Error(`standalone fixture did not install runIsolationProbe on ${page.url()}`);
      }
    } else {
      await page.goto(`http://localhost:1420/tests/mail-content/harness.html`, {
        waitUntil: 'domcontentloaded',
        timeout: 120_000,
      });
      await page.evaluate((content) => window.setMail({ content }), attacks);
      await page.waitForTimeout(400);
    }

    const beforeAuthorize = [...requested];
    const probe = await page.evaluate(() => window.runIsolationProbe());
    const parentHasRealTauri = await page.evaluate(() => {
      const bridge = window.__TAURI_INTERNALS__ || window.__TAURI__;
      if (!bridge || typeof bridge.invoke !== 'function') return false;
      // Mock fallback only defines invoke; real Tauri exposes more internals.
      return typeof bridge.transformCallback === 'function'
        || !!bridge.metadata
        || typeof bridge.unregisterCallback === 'function';
    });

    let bypassRequests = [];
    if (mode === 'release') {
      requested.length = 0;
      await mountStandaloneFixture(page, { bypassSanitizer: true });
      await page.waitForTimeout(400);
      bypassRequests = [...requested];
      // Frame CSP must prevent a successful remote image load; aborted/blocked
      // request events are acceptable as long as scripts still do not run.
      const bypassProbe = await page.evaluate(() => window.runIsolationProbe());
      if (bypassProbe.parentInjectedScriptRan || bypassProbe.mailScriptMarker || bypassProbe.ipcProbeResult === 'invoked-ok') {
        fail('bypass fixture allowed script/IPC in release WebView');
      }
    }

    const report = await assertProbe(probe, beforeAuthorize, parentHasRealTauri);
    report.bypassSanitizerRemoteRequests = bypassRequests;
    report.generatedAt = new Date().toISOString();
    fs.writeFileSync(reportPath, JSON.stringify(report, null, 2));
    console.log(JSON.stringify(report, null, 2));
    console.error(`[harness] wrote ${reportPath}`);
    if (report.status !== 'ok') process.exitCode = 1;
  } finally {
    if (process.env.POSTLET_KEEP_OPEN === '1') {
      console.error('POSTLET_KEEP_OPEN=1 — leaving app running (app data still stashed until process exit)');
      return;
    }
    try { await browser?.close(); } catch { /* ignore */ }
    await killTree(app);
    await delay(500);
    restoreAppDatabase(stash);
  }
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
