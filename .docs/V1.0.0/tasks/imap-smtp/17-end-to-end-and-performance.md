# Task 17：跨协议验收和邮箱规模基准

- 状态：已完成（真实邮箱/OAuth 未验证，已标注）
- 实现备注：新增 `src-tauri/tests/end_to_end_acceptance.rs`（同位置幂等/跨账号独立、Trash 目标、UID 缺口差集）与 `mailbox_scale_baseline.rs`（2k/20k/100k 差集基线与分页收敛）。命令、结果、平台与未验证项汇总见 [imap-smtp-acceptance.md](../../imap-smtp-acceptance.md)。
- 对应问题：I-01、I-06～I-08、I-10～I-19、A-01～A-06；明确排除 I-09
- 前置任务：01～16

## 修改位置

- `src-tauri/tests/、tests/mail-content/`：补充跨仓储、协议假服务器、失败注入和真实 Tauri 验收；测试基于实际最终契约。
- `src-tauri/src/infrastructure/imap/sync.rs、src-tauri/src/domain/services/imap_sync_policy.rs`：对照现有 UID 全集差集基线测量大邮箱，不在未发现瓶颈时引入高水位优化。
- `.docs/V1.0.0/tasks/imap-smtp/README.md`：逐项记录执行命令、结果、平台及未验证项；每个任务通过后单独提交并更新状态。

## 实施步骤

1. 使用隔离测试邮箱和受控服务器跑 Graph/IMAP 相同位置幂等、跨账号/文件夹独立、移动和错误恢复；永久删除用独立测试数据。
2. 覆盖附件字节一致、发送接受与结果不确定、两种 Sent 策略、离线重启、同步批次失败、漏事件恢复和登录退出。
3. 为代表性邮箱规模记录 UID 数量、耗时和内存；只有实测达到项目约定瓶颈才设计增量发现，并与全集差集对照崩溃/失败恢复。
4. 复验已提交的 UIDPLUS 安全删除、UIDVALIDITY、凭据加密、连接取消和分层依赖；不重做已实现功能。

## 验收

- I-15 只做已有连接改造的回归：登录 deadline 不得复用为整段同步的 socket 超时；登录成功后使用独立 session IO 预算/操作 deadline，取消后旧连接实际退出才放行。不得借本卡重写连接层。
- I-09 的通用 OAuth2/XOAUTH2 完整生命周期仍暂缓，不属于本卡修复或上线验收；通用 OAuth2 输入拒绝由 Task 01 验收，Graph OAuth 保留。

- Rust 全套、前端构建、安全测试和真实 Tauri 测试按目标平台执行；系统钥匙串测试使用隔离条目并清理。
- 形成按原问题编号逐项对应的证据；真实 OAuth、邮箱服务器或其他平台未执行时如实标注，不宣称全部完成。

完成并通过相关验证后，将本任务涉及的代码和测试单独提交；更新总览状态。

