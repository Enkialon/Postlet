import { test, expect } from '@playwright/test';
import { readFileSync } from 'node:fs';

const security = JSON.parse(readFileSync(new URL('../../src-tauri/tauri.conf.json', import.meta.url))).app.security;
let pageErrors;
test.beforeEach(async ({ page }, info) => {
  pageErrors = [];
  page.on('pageerror', error => pageErrors.push(error.message));
  await page.route('**/tests/mail-content/harness.html', async route => {
    const response = await route.fetch();
    await route.fulfill({ response, headers: {
      ...response.headers(),
      'content-security-policy': info.project.name === 'production-csp' ? security.csp : security.devCsp,
    } });
  });
});
test.afterEach(() => expect(pageErrors).toEqual([]));

const attacks = `
<p style="color:red;background-image:url(https://tracker.invalid/css)">safe body</p>
<script>parent.__TAURI_INTERNALS__.invoke('delete_messages')</script>
<img src="https://tracker.invalid/image" srcset="https://tracker.invalid/srcset 2x" onerror="parent.compromised=true">
<div background="https://tracker.invalid/background" style="background:url(https://tracker.invalid/shorthand)">CSS</div>
<style>@import url(https://tracker.invalid/import); @font-face{font-family:x;src:url(https://tracker.invalid/font)} body{background-image:url(https://tracker.invalid/style)}</style>
<iframe src="https://tracker.invalid/frame"></iframe><object data="https://tracker.invalid/object"></object>
<link rel="stylesheet" href="https://tracker.invalid/link"><meta http-equiv="refresh" content="0;url=https://tracker.invalid/refresh">
<form action="https://tracker.invalid/form"><input type="image" src="https://tracker.invalid/input"></form>
<svg><a xlink:href="javascript:parent.compromised=true">svg</a></svg>
<math><mtext><table><mglyph><style><!--</style><img title="--><img src=https://tracker.invalid/mutation onerror=parent.compromised=true>"></table></mtext></math>
<img src="cid:foreign-attachment"><img src="file:///C:/secret"><img src="asset://localhost/secret"><img src="data:image/svg+xml,<svg onload='parent.compromised=true'/>">
<a href="javascript:parent.compromised=true">bad link</a><a href="data:text/html,bad">data link</a>
<a href="https://example.com/path?q=1">website</a><a href="mailto:user@example.com">mail link</a>
`;

test('parsing and default rendering do not request remote HTML or CSS resources', async ({ page }) => {
  const requested = [];
  await page.route('https://tracker.invalid/**', route => { requested.push(route.request().url()); return route.abort(); });
  await page.goto('/tests/mail-content/harness.html');
  await page.evaluate(content => window.setMail({ content }), attacks);
  const frame = page.frameLocator('iframe');
  await expect(frame.locator('body')).toContainText('safe body');
  await expect(frame.locator('script, iframe, object, form, svg, math, link')).toHaveCount(0);
  await expect(frame.locator('img[src]')).toHaveCount(0);
  await expect(frame.locator('p')).toHaveCSS('color', 'rgb(255, 0, 0)');
  await expect(frame.locator('[style*="url"]')).toHaveCount(0);
  await expect(page.locator('iframe')).toHaveAttribute('sandbox', 'allow-same-origin');
  await page.waitForTimeout(300);
  expect(requested).toEqual([]);
  expect(await page.evaluate(() => window.compromised)).toBeUndefined();
});

test('remote image permission is limited to the current message and can be revoked', async ({ page }) => {
  const requested = [];
  await page.route('https://tracker.invalid/**', route => {
    requested.push(route.request().url());
    return route.fulfill({ contentType: 'image/gif', body: Buffer.from('R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7', 'base64') });
  });
  await page.goto('/tests/mail-content/harness.html');
  await page.evaluate(content => window.setMail({ content }), attacks);
  await expect(page.frameLocator('iframe').locator('body')).toContainText('safe body');
  await page.getByRole('button', { name: '加载此邮件的远程图片' }).click();
  await expect.poll(() => requested.length).toBe(1);
  expect(requested).toEqual(['https://tracker.invalid/image']);
  await page.getByRole('button', { name: '屏蔽远程图片' }).click();
  await expect(page.frameLocator('iframe').locator('img[src]')).toHaveCount(0);
  await page.getByRole('button', { name: '加载此邮件的远程图片' }).click();
  await expect(page.frameLocator('iframe').locator('img[src]')).toHaveCount(1);
  // Identical content in another message must not inherit consent.
  await page.evaluate(() => window.setMail({ key: 'account:message-2' }));
  await expect(page.getByRole('button', { name: '加载此邮件的远程图片' })).toBeVisible();
  await expect(page.frameLocator('iframe').locator('img[src]')).toHaveCount(0);
});

test('links use the parent Port; dangerous links cannot navigate or open windows', async ({ page }) => {
  await page.goto('/tests/mail-content/harness.html');
  await page.evaluate(content => window.setMail({ content }), attacks);
  const frame = page.frameLocator('iframe');
  await frame.getByText('website', { exact: true }).click();
  await frame.getByText('mail link', { exact: true }).click();
  await frame.getByText('bad link', { exact: true }).click();
  await frame.getByText('data link', { exact: true }).click();
  expect(await page.evaluate(() => window.opened)).toEqual(['https://example.com/path?q=1', 'mailto:user@example.com']);
  expect(page.context().pages()).toHaveLength(1);
  expect(page.frames()[1].url()).toBe('about:srcdoc');
});

test('sandbox and CSP block scripts and resource loads even if sanitization is bypassed', async ({ page }) => {
  const requested = [];
  await page.route('https://tracker.invalid/**', route => { requested.push(route.request().url()); return route.abort(); });
  await page.goto('/tests/mail-content/harness.html');
  await page.locator('iframe').waitFor();
  // Deliberately replace only the already-sanitized body to test the independent boundary.
  await page.evaluate(() => {
    const frame = document.querySelector('iframe');
    frame.srcdoc = frame.srcdoc.replace(/<body>[\s\S]*<\/body>/, `<body>
      <script>parent.__TAURI_INTERNALS__.invoke('delete_messages')<\/script>
      <img src="https://tracker.invalid/bypass" onerror="parent.compromised=true">
      <style>body{background:url(https://tracker.invalid/css-bypass)}</style>
      boundary test</body>`);
  });
  await expect(page.frameLocator('iframe').locator('body')).toContainText('boundary test');
  await page.waitForTimeout(300);
  expect(requested).toEqual([]);
  expect(await page.evaluate(() => window.compromised)).toBeUndefined();
});
