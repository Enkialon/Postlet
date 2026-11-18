# Task 16：在真实 Tauri WebView 验证 HTML 隔离

- 状态：已完成（含一项已评估并记录的残留）
- 实测结论（WebView2 / Windows）：
  - 生产 `postlet.exe` + 镜像 MailBodyView 的沙箱固件：`allowsScripts=false`、`parentInjectedScriptRan=false`、`ipcProbeResult=no-ipc-side-effect`、授权前无任何远程请求；见 `tests/mail-content/last-tauri-webview-report-release.json`。
  - Playwright（Chromium，dev/production CSP 两套）：清洗、默认屏蔽远程资源、逐邮件授权、危险链接不外开、sandbox 阻止脚本与资源加载，8 项全通过。
- 残留（已评估，不做进一步改动）：iframe 使用 `allow-same-origin` 且无 `allow-scripts`，因此子上下文仍能看到同源对象与 Tauri bridge，但沙箱与文档 CSP（`script-src 'none'`）禁止任何脚本执行，实测无法产生 IPC 副作用。本轮曾尝试改为跨源 `allow-scripts` + CSP nonce 的桥接方案，桥接脚本在 Chromium/WebView2 下未稳定执行，导致链接与高度功能回退，故保留现方案并记录该限制。
- 未验证平台：macOS WKWebView、Linux WebKitGTK；打包安装器 UI 冒烟未自动化。
- 对应问题：I-19
- 前置任务：01
- 剩余范围：基线已有清洗、iframe/CSP、默认远程资源屏蔽及浏览器测试；本卡只做真实 Tauri 开发/生产验收，并按实测缺陷作必要修正，不重新实现整套渲染。

## 修改位置

- `src/shared/mail-content/{mailDocument.ts,MailBodyView.vue}`：保留已实现的清洗、默认屏蔽远程资源、逐邮件授权和外链校验；针对实机结果修正上下文隔离。
- `src-tauri/tauri.conf.json、src-tauri/capabilities/default.json、src-tauri/permissions/app-ipc.toml`：检查开发和生产 CSP、主窗口与邮件子上下文的实际 capability 和 IPC 授权。
- `tests/mail-content/、Tauri 实机测试入口`：扩展网络捕获及子上下文 IPC 拒绝测试，分别记录开发和生产包结果。

## 实施步骤

1. 在真实 Tauri/WebView 中加载恶意 HTML，捕获解析、展示和授权前的图片、CSS、字体、iframe 与本地资源请求。
2. 验证邮件子上下文不能调用文件、删除、发信等 IPC。若同源 iframe 获得能力，改为不共享主窗口权限的隔离上下文；高度和链接通过受校验消息交给父页面处理。
3. 父页面只允许合法的 http、https、mailto 链接经 PlatformPort 打开；不为邮件内容增加主窗口权限。

## 验收

- 脚本、事件、危险 URL、畸形 HTML、CSS、CID、本地资源、授权撤销和跨邮件重置；默认无远程请求。
- 开发与生产安装包分别验证 IPC 拒绝和 CSP；记录实际平台及未验证平台。

完成并通过相关验证后，将本任务涉及的代码和测试单独提交；更新总览状态。

