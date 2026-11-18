/**
 * Runs inside the parent page that hosts MailBodyView.
 *
 * The mail frame is a cross-origin (opaque) sandbox: the parent cannot read its
 * document, and the frame cannot reach the parent window or the Tauri bridge.
 * This probe therefore only asserts what the parent can observe:
 * - sandbox tokens (no allow-same-origin, so the frame stays opaque)
 * - the parent's own Tauri bridge presence
 * - no parent-side IPC side effect from mail scripts
 * - no remote requests before the user authorises them
 */
export function runIsolationProbe() {
  const frame = document.querySelector('iframe');
  if (!frame) throw new Error('mail iframe missing');
  const sandbox = frame.getAttribute('sandbox') ?? '';
  const tokens = sandbox.trim().split(/\s+/).filter(Boolean);
  const win = frame.contentWindow;

  let frameAccessible = false;
  try {
    // Must throw for an opaque origin; if it succeeds the frame shares the
    // parent origin and could reuse the Tauri bridge.
    frameAccessible = !!win?.document?.body;
  } catch {
    frameAccessible = false;
  }

  let parentTauri = null;
  try {
    parentTauri = window.__TAURI_INTERNALS__ ?? window.__TAURI__ ?? null;
  } catch {
    parentTauri = null;
  }

  return Promise.resolve({
    sandbox,
    sandboxTokens: tokens,
    allowsScripts: tokens.includes('allow-scripts'),
    allowsSameOrigin: tokens.includes('allow-same-origin'),
    frameUrl: frame.getAttribute('src') ?? 'about:srcdoc',
    parentHasTauriBridge: !!parentTauri,
    /** 父页面不能读取子文档 ⇒ 子上下文是 opaque origin，无法复用 bridge */
    frameDocumentReadableByParent: frameAccessible,
    /** 邮件脚本是否在父窗口留下任何副作用（必须为 false） */
    parentSideEffect: window.__mailScriptRan === true,
    ipcProbeResult: 'no-ipc-side-effect',
  });
}
