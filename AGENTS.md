# Postlet — Agent 指南

## 1. 核心工程原则

- **第一性原则**：从实际问题和业务本质出发，不受既有设计模式、架构惯例束缚。
- **KISS**：正确性、安全性优先；在满足约束的方案中，始终选择最简单的。
- **YAGNI**：不为假设的未来需求、理论上的通用性或极端场景提前设计。
- **简化优先**：遇到复杂问题，先考虑能否简化模型、消除问题，而不是增加抽象和补偿机制。
- **最小改动**：优先复用现有代码、数据结构和依赖；不增加无明确必要性的层、接口或框架。
- **设计约束**：遵循已确认的设计文档。发现更简单的方案可以提出，但不得擅自改变业务规则或扩大任务范围。

**核心准则：任何新增复杂度都必须证明其必要性。代码量最少不等于整体设计最简单。**

## 2. V1 阶段约束

本项目尚未上线，目标是实现干净、正确、可维护的 V1。

- 不考虑旧安装、数据库、IPC、DTO、配置的升级兼容。
- 错误设计直接修正，不保留兼容双轨、废弃别名或临时垫片。
- 必要时允许重建开发数据库，不为迁移历史数据增加长期代码。
- 不擅自增加 V1 范围或实现设计文档明确暂缓的功能。

## 3. 产品与业务模型

Postlet 是基于 **Tauri 2** 的联系人/会话优先桌面邮件客户端。

- **Session 是一等公民**；Contact/Group 是关联实体或投影，文件夹不是默认主导航。
- Message 属于一个 Account、一个 Session，最多对应一个远端位置。
- 不同账号、不同远端位置的 Message 独立；同位置重复同步必须幂等。
- 移动按旧位置移除、新位置新增处理，不进行跨位置内容去重或身份合并。
- 支持 Microsoft Graph 和通用 IMAP/SMTP，按账号协议配置路由，不根据邮箱品牌硬编码。
- 以标准 IMAP/RFC 能力为统一业务基线；Graph 专有身份属性不得参与业务身份、会话归属或去重规则。

## 4. 技术栈与架构

- 桌面：Tauri 2；前端：Vue 3。
- Graph：Rust `graph-rs-sdk`。
- IMAP/SMTP：Rust infrastructure 中的协议模块。
- 存储：SQLite。
- 所有邮件协议访问均在 Rust，前端不直接请求 Graph/IMAP/SMTP。

### Rust 分层

```text
src-tauri/
  domain/          # 实体、值对象、纯业务规则
  application/     # 用例编排、事务控制
  infrastructure/  # SQLite、Graph、IMAP、SMTP、OAuth
  commands/        # Tauri IPC 薄胶水
  runtime/         # 后台调度、事件分发
```

约束：

- domain 无 IO，不依赖 Tauri、数据库或协议库。
- application 调用具体 infrastructure 模块函数；不依赖 runtime 或 Tauri AppState。
- infrastructure 不反向依赖 application。
- Repository/UseCase 使用模块函数，不引入 Repository、TransactionManager trait 对象。
- application 可以使用 SqlitePool，并显式管理跨仓储事务。
- 协议差异限制在 infrastructure，不复制整套业务架构。
- Graph 专有字段不得进入统一领域模型或通用数据库结构。
- 不额外创建 Rust `interfaces/` 层；commands 只处理 IPC。

### 前端 Port / Adapter

```text
UI / Features
      ↓
Ports（接口）
      ↑
Adapters（Tauri invoke）
```

- UI、hooks、stores 依赖 Port，不直接调用 `invoke`。
- Tauri Adapter 实现 Port，统一处理 IPC 调用。
- 优先复用已有 Port/Adapter，不为每个功能创建新接口。
- commands 不包含业务规则或协议实现。

## 5. Agent 工作规则

1. 修改前先阅读相关代码和设计文档，理解现有实现。
2. 先找最简单的正确方案，不机械套用 DDD、设计模式或分层流程。
3. 只修改任务涉及的部分，不顺手扩展功能或重构无关模块。
4. 仅在确实涉及某一层时修改该层，不为遵循固定流程而创建无用代码。
5. 保留必要的错误处理、数据安全和失败恢复，不以简单为由牺牲正确性。
6. 完成后验证相关功能；未验证的行为不得宣称已通过。
7. 如发现设计存在问题，说明更简单的替代方案及影响，不擅自推翻已确认的业务不变量。

## 6. 非目标

- 自建邮件服务器或服务端中转。
- 以文件夹为中心的传统邮件 UI。
- 按邮箱品牌维护独立业务架构。
- 未上线 V1 的历史版本兼容。
- 没有明确需求的通用框架和提前扩展设计。

**最终原则：解决真实问题，选择最简单的正确实现。不要为了架构而架构。**
