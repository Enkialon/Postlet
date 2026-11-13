import DOMPurify from 'dompurify';

/** 允许列表净化 HTML 正文；禁止脚本 / 事件 / 表单 / 外嵌等 */
export function sanitizeMailHtml(html: string): string {
  return DOMPurify.sanitize(html, {
    USE_PROFILES: { html: true },
    FORBID_TAGS: [
      'script',
      'iframe',
      'object',
      'embed',
      'form',
      'input',
      'button',
      'textarea',
      'select',
      'base',
      'link',
      'meta',
    ],
    FORBID_ATTR: [
      'style',
      'srcset',
      'onclick',
      'onerror',
      'onload',
      'onmouseover',
      'onfocus',
      'onblur',
    ],
    ALLOWED_URI_REGEXP:
      /^(?:(?:https?|mailto):|[^a-z]|[a-z+.-]+(?:[^a-z+.\-:]|$))/i,
    ADD_ATTR: ['target', 'rel'],
  });
}

/** 校验可交系统浏览器打开的外链 */
export function isSafeExternalHttpUrl(url: string): boolean {
  try {
    const u = new URL(url.trim());
    return u.protocol === 'http:' || u.protocol === 'https:';
  } catch {
    return false;
  }
}

/** 隔离 iframe 文档包装；默认禁止网络 */
export function wrapSandboxedMailDocument(sanitizedHtml: string): string {
  const csp =
    "default-src 'none'; img-src data:; style-src 'unsafe-inline'; font-src 'none'; script-src 'none'; connect-src 'none'; frame-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'";
  return `<!DOCTYPE html><html><head><meta charset="utf-8" /><meta http-equiv="Content-Security-Policy" content="${csp}" /><base target="_blank" rel="noopener noreferrer" /></head><body>${sanitizedHtml}</body></html>`;
}
