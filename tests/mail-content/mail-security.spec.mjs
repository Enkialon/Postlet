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

test('parsing and rendering load no remote HTML, CSS or font resources', async ({ page }) => {
  const requested = [];
  await page.route('https://tracker.invalid/**', route => { requested.push(route.request().url()); return route.abort(); });
  await page.goto('/tests/mail-content/harness.html');
  await page.evaluate(content => window.setMail({ content }), attacks);
  const frame = page.frameLocator('iframe');
  await expect(frame.locator('body')).toContainText('safe body');
  await expect(frame.locator('script, iframe, object, form, svg, math, link')).toHaveCount(0);
  await expect(frame.locator('p')).toHaveCSS('color', 'rgb(255, 0, 0)');
  await expect(frame.locator('[style*="url"]')).toHaveCount(0);
  await expect(page.locator('iframe')).toHaveAttribute('sandbox', 'allow-same-origin');
  // 除允许的 http(s) 图片外，cid/file/asset/data 等源一律不保留
  await expect(frame.locator('img[src^="cid:"], img[src^="file:"], img[src^="asset:"], img[src^="data:"]')).toHaveCount(0);
  await page.waitForTimeout(300);
  expect(requested.filter(url => /css|import|font|frame|object|refresh|link/.test(url))).toEqual([]);
  expect(await page.evaluate(() => window.compromised)).toBeUndefined();
});

test('normal mail loads permitted images directly without any authorization banner', async ({ page }) => {
  const requested = [];
  await page.route('https://tracker.invalid/**', route => {
    requested.push(route.request().url());
    return route.fulfill({ contentType: 'image/gif', body: Buffer.from('R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7', 'base64') });
  });
  await page.goto('/tests/mail-content/harness.html');
  await page.evaluate(content => window.setMail({ content }), attacks);
  await expect(page.frameLocator('iframe').locator('body')).toContainText('safe body');
  await expect(page.getByRole('button', { name: '加载此邮件的远程图片' })).toHaveCount(0);
  await expect(page.getByRole('button', { name: '屏蔽远程图片' })).toHaveCount(0);
  await expect(page.frameLocator('iframe').locator('img[src="https://tracker.invalid/image"]')).toHaveCount(1);
  await expect.poll(() => requested.includes('https://tracker.invalid/image')).toBe(true);
  // 另一封邮件仍按同样规则渲染：无提示，也不继承上一封的任何状态
  await page.evaluate(() => window.setMail({ key: 'account:message-2' }));
  await expect(page.getByRole('button', { name: '加载此邮件的远程图片' })).toHaveCount(0);
  await expect(page.frameLocator('iframe').locator('img[src="https://tracker.invalid/image"]')).toHaveCount(1);
});

test('junk mail renders literal text without an iframe, link or remote request', async ({ page }) => {
  const requested = [];
  await page.route('https://tracker.invalid/**', route => { requested.push(route.request().url()); return route.abort(); });
  await page.goto('/tests/mail-content/harness.html');
  await page.evaluate(content => window.setMail({ content }), attacks);
  await expect(page.locator('iframe')).toHaveCount(1);
  // 切到垃圾邮件：旧 iframe 必须卸载，HTML 源码按文字显示
  await page.evaluate(() => window.setMail({ folderRole: 'junk' }));
  await expect(page.locator('iframe')).toHaveCount(0);
  await expect(page.getByText('safe body', { exact: false })).toBeVisible();
  await expect(page.locator('p', { hasText: 'safe body' })).toHaveCount(0);
  await expect(page.locator('img, a')).toHaveCount(0);
  // 普通渲染阶段已允许的图片请求不计入；切到垃圾后面不得新增请求
  requested.length = 0;
  await page.waitForTimeout(300);
  expect(requested).toEqual([]);
  expect(await page.evaluate(() => window.compromised)).toBeUndefined();
  // 纯文本正文在垃圾邮件中同样只按文本展示
  await page.evaluate(() => window.setMail({ content: '<b>plain</b>', contentType: 'text' }));
  await expect(page.getByText('<b>plain</b>', { exact: true })).toBeVisible();
  // 移出垃圾后恢复普通 HTML 隔离渲染
  await page.evaluate(() => window.setMail({ folderRole: null, contentType: 'html' }));
  await expect(page.locator('iframe')).toHaveCount(1);
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
  // CSP 独立于净化：脚本不执行；只允许 http(s) 图片，其他资源类型仍需被阻断
  expect(requested.filter(url => /font|frame|object|refresh|link|import\.css/.test(url))).toEqual([]);
  expect(await page.evaluate(() => window.compromised)).toBeUndefined();
});
