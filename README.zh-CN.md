# Postlet

**Postlet** 是基于 [Tauri 2](https://tauri.app/) 的桌面邮件客户端。  
以**联系人 / 会话**为一等公民，而不是传统文件夹收件箱优先。

[English](./README.md)

## 功能

- **会话优先界面** — 像聊天一样浏览与某人或某群的邮件往来
- 同时支持 **Microsoft Graph（Outlook）** 与 **IMAP + SMTP**
- 按账号协议字段路由（`receive_protocol` / `send_protocol`），不按服务商品牌硬编码
- 本地 SQLite；Graph / IMAP 流量只走 Rust 后端
- 邮件软删除（可读「显示已删除」）；隐藏会话不删邮件
- 登录后自选同步文件夹（默认不勾选）

## 技术栈

| 层 | 选型 |
|---|---|
| 桌面壳 | Tauri 2 |
| UI | Vue 3 + TypeScript + Vite + Tailwind |
| Graph | `graph-rs-sdk`（仅 Rust） |
| IMAP / SMTP | Rust（`imap` / `lettre`） |
| 数据库 | SQLite（SQLx） |
| 许可 | [GPL-3.0-only](./LICENSE) |

## 环境要求

- Node.js 20+ 与 [pnpm](https://pnpm.io/)
- Rust stable（Tauri）
- 各平台 Tauri 依赖（见 [Tauri 前置条件](https://v2.tauri.app/start/prerequisites/)）

## 开发

```bash
pnpm install
pnpm tauri dev
```

仅前端：

```bash
pnpm dev
```

## 构建

```bash
pnpm tauri build
```

安装包输出在 `src-tauri/target/release/bundle/`。

## 国际化

界面文案以中文为源语言。Vite 插件在编译期将模板静态中文转为 `$t(...)`。  
抽取新词条（只追加缺失 key，不覆盖已有译文）：

```bash
pnpm i18n:extract
```

语言文件在 `src/i18n/locales/`。默认跟随系统语言，可在设置中覆盖。

## 发布

推送 `v*` 标签会触发 [`.github/workflows/release.yml`](./.github/workflows/release.yml)，在 **Windows** 与 **macOS**（Apple Silicon + Intel）上打包并挂到 GitHub Release。

## 许可证

Copyright (C) 2026 本澈  

本程序为自由软件：你可以按自由软件基金会发布的 **GNU 通用公共许可证第 3 版**（或你选择的后续版本）的条款重新发布和/或修改；包元数据中声明为 `GPL-3.0-only`。详见 [LICENSE](./LICENSE)。
