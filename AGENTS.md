# Outlook 联系人会话优先邮件客户端 — Agent 指南

## 产品定位

基于 **Tauri 2** 的桌面邮件客户端：UI 以**联系人 / 会话（conversation）**为一等公民，而不是传统文件夹收件箱优先。

- 仅对接 **Microsoft Outlook / Microsoft Graph**
- 邮件、联系人、会话列表均围绕「与某人的往来」组织

## 技术栈硬约束

| 层 | 选型 | 禁止 |
|---|---|---|
| 桌面壳 | Tauri 2 | Electron / 纯 Web 壳 |
| Graph 客户端 | `graph-rs-sdk`（Rust） | 前端直连 Graph、其他 Graph SDK |
| 邮件后端 | Outlook / Graph only | Gmail、IMAP 通用、多厂商抽象「先做通用」 |
| Rust 架构 | DDD | 大泥球 / 命令里直接堆 Graph 调用 |
| 前端 ↔ Rust | 适配器：先接口，再实现调 RS 桥 | UI 组件直接 `invoke` 散落业务 |

## 入站边界（桌面应用）

本项目是桌面应用：**前端（Port + Adapter）就是入站适配器**，不在 Rust 侧再单独建 `interfaces/` 入站层。

## Rust DDD 分层

```
src-tauri/
  domain/          # 实体、值对象、领域服务、仓储/端口接口（纯 Rust，无 Graph/Tauri）
  application/     # 用例 / 应用服务（编排领域，依赖端口）
  infrastructure/  # graph-rs-sdk、本地存储、OAuth 等端口实现（出站）
  commands/        # 可选：Tauri IPC 薄胶水（非 DDD 层，仅 serde/invoke 桥接）
```

规则：

1. **依赖方向**：`commands`（薄）→ `application` → `domain` ← `infrastructure`
2. **domain 零 IO**：不得依赖 `graph-rs-sdk`、`tauri`、文件系统、网络
3. **端口在 domain（或 application 边界）定义**；Graph 调用只出现在 `infrastructure`
4. **会话优先模型**：领域核心是 `Contact` / `Conversation` / `Message`，文件夹只是可选投影，不是主导航模型
5. 新增能力顺序：领域概念 → 端口 → 用例 → 基础设施实现 →（如需）Tauri command 胶水
6. **不设 `interfaces/`**：真正的入站在前端；Rust `commands/` 只做 IPC，不算业务入站层

## 前端适配器约定（真正的入站）

前端不直接把业务绑在 Tauri `invoke` 上，统一走适配器：

```
ui / features
    ↓ 依赖
ports（接口：TypeScript interface / 抽象）
    ↓ 由实现类满足
adapters（实现类：调用 RS 桥 / invoke）
```

强制顺序：

1. **先写接口**（如 `MailPort`、`ContactPort`、`ConversationPort`）
2. **再写实现类**（如 `TauriMailAdapter`），内部调用 RS 桥
3. UI / hooks / stores **只依赖接口**，通过 DI / 工厂注入实现

禁止：

- 页面/组件里直接 `invoke('xxx')` 拼业务
- 为「省事」跳过接口，让 feature 绑定具体 adapter
- 在前端再包一层 Graph REST（所有 Graph 流量经 Rust）

## RS 桥（Tauri commands，IPC 胶水）

- 非 DDD 分层：仅参数校验 → 调 application 用例 → 映射可序列化 DTO
- 不在 command 内写 Graph API 细节或业务规则
- 错误：统一映射到前端可展示的结构

## 实现时检查清单

- [ ] 新功能是否从领域/端口开始，而不是从 Graph endpoint 或 UI 开始？
- [ ] Graph 调用是否只在 `infrastructure` + `graph-rs-sdk`？
- [ ] 前端是否先有 Port 接口，再有调用 RS 桥的 Adapter？
- [ ] UI 是否按联系人/会话导航，而非文件夹优先？
- [ ] domain 是否仍无 Tauri / Graph 依赖？

## 非目标（当前不做）

- 多邮箱厂商统一协议层
- 服务端中转 / 自建邮件服务器
- 以文件夹树为默认信息架构
