import DOMPurify from 'dompurify';

// These properties cannot load resources; shorthand backgrounds, content,
// custom properties and stylesheets are deliberately excluded.
const presentationProperties = new Set([
  'color', 'background-color', 'font-family', 'font-size', 'font-weight', 'font-style',
  'text-align', 'text-decoration', 'text-indent', 'line-height', 'letter-spacing',
  'white-space', 'word-break', 'overflow-wrap', 'vertical-align',
  'margin', 'margin-top', 'margin-right', 'margin-bottom', 'margin-left',
  'padding', 'padding-top', 'padding-right', 'padding-bottom', 'padding-left',
  'border', 'border-top', 'border-right', 'border-bottom', 'border-left',
  'border-color', 'border-style', 'border-width', 'border-collapse', 'border-spacing',
  'width', 'max-width', 'min-width', 'height', 'max-height', 'min-height',
]);

export function externalMailUrl(raw: string, allowMailto = true): string | null {
  try {
    const url = new URL(raw);
    if (url.protocol === 'http:' || url.protocol === 'https:' || (allowMailto && url.protocol === 'mailto:')) {
      return url.href;
    }
  } catch { /* Relative and malformed URLs are not mail resources. */ }
  return null;
}

export function createMailDocument(content: string, allowRemoteImages: boolean): string {
  // A template's owner document is inert. Raw markup never enters the live
  // application document, including during parsing and sanitization.
  const template = document.createElement('template');
  const body = template.content.ownerDocument.createElement('div');
  template.content.append(body);
  if (!DOMPurify.isSupported) {
    body.textContent = content;
  } else {
    body.innerHTML = content;
    try {
      DOMPurify.sanitize(body, {
        IN_PLACE: true,
        ALLOWED_TAGS: [
          'div', 'p', 'span', 'br', 'hr', 'b', 'i', 'strong', 'em', 'u', 's', 'small',
          'sub', 'sup', 'pre', 'code', 'blockquote', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6',
          'ul', 'ol', 'li', 'dl', 'dt', 'dd', 'table', 'thead', 'tbody', 'tfoot', 'tr',
          'td', 'th', 'caption', 'colgroup', 'col', 'a', 'img',
        ],
        ALLOWED_ATTR: ['href', 'src', 'alt', 'title', 'width', 'height', 'colspan', 'rowspan', 'align', 'dir', 'lang', 'style'],
        ALLOW_DATA_ATTR: false,
        ALLOW_ARIA_ATTR: false,
      });
    } catch {
      body.textContent = content;
    }
  }

  for (const element of body.querySelectorAll<HTMLElement>('[style]')) {
    for (const property of Array.from(element.style)) {
      if (!presentationProperties.has(property)) element.style.removeProperty(property);
    }
  }
  for (const image of body.querySelectorAll('img')) {
    const src = externalMailUrl(image.getAttribute('src') ?? '', false);
    image.removeAttribute('src');
    image.setAttribute('referrerpolicy', 'no-referrer');
    if (src && allowRemoteImages) image.setAttribute('src', src);
  }
  for (const anchor of body.querySelectorAll('a')) {
    const href = externalMailUrl(anchor.getAttribute('href') ?? '');
    anchor.removeAttribute('href');
    if (href) {
      // Keep the real destination out of browser navigation. The parent opens
      // it through the platform Port only after a user click.
      anchor.setAttribute('href', '#');
      anchor.setAttribute('data-mail-link', href);
    }
  }

  const csp = `default-src 'none'; script-src 'none'; style-src 'unsafe-inline'; img-src ${allowRemoteImages ? 'https: http:' : "'none'"}; base-uri 'none'; form-action 'none'`;
  return `<!doctype html><html><head><meta http-equiv="Content-Security-Policy" content="${csp}"><meta name="referrer" content="no-referrer"><style>
    html { color-scheme: light; }
    body { margin: 0; padding: 2px; color: #273340; background: white; font: 14px/1.65 system-ui, sans-serif; overflow-wrap: anywhere; }
    p { margin: 0 0 .85em; } h1,h2,h3 { line-height: 1.3; }
    img { max-width: 100%; height: auto; } table { max-width: 100%; }
    blockquote { margin: .5em 0; padding-left: .85em; border-left: 3px solid #ddd; color: #596776; }
    a { color: #3b6d9a; } pre { white-space: pre-wrap; }
  </style></head><body>${body.innerHTML}</body></html>`;
}

/** Called by trusted parent code; the frame itself cannot execute scripts. */
export function bindMailLinks(frame: HTMLIFrameElement, open: (url: string) => Promise<void>): void {
  const doc = frame.contentDocument;
  if (!doc) return;
  const onClick = (event: MouseEvent) => {
    event.preventDefault();
    if (event.button !== 0 && event.button !== 1) return;
    const target = event.target as Element | null;
    const href = target?.closest?.('a')?.getAttribute('data-mail-link');
    const safe = href ? externalMailUrl(href) : null;
    if (safe) void open(safe);
  };
  doc.addEventListener('click', onClick);
  doc.addEventListener('auxclick', onClick);
}
