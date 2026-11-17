# Task 07：分类 SMTP 错误与不确定结果

- 状态：已完成
- 实现备注：明文 TLS 在建立连接前拒绝（有零网络访问测试）。`domain::mail` 新增 `SendOutcome`/`SendStage`/`SendResult` 业务语义；SMTP 按状态码与 lettre 错误类型区分认证拒绝、4xx 可重试、5xx 永久、TLS 不可用不重试、超时/无状态码判为结果不确定；Graph 按 401/403、429、4xx、5xx 分类。DATA 后断线阶段无法从 lettre 精确还原，统一按「不确定」处理，不猜测未接受。
- 对应问题：I-18、I-08
- 前置任务：01、06
- 剩余范围：基线已有 lettre 发送及 TLS/STARTTLS 路径、通用 OAuth2 连接前拒绝；本卡只补发送结果分类和 Plain 拒绝，不重建配置系统。

## 修改位置

- `src-tauri/src/infrastructure/smtp/send.rs、src-tauri/src/infrastructure/error.rs`：替换统一 retryable=true；返回明确的认证拒绝、暂时拒绝、永久拒绝、服务器接受和结果不确定事实。
- `src-tauri/src/domain/mail.rs`：固定在现有统一邮件事实模块定义发送结果及阶段的最小纯数据类型，供 Graph/SMTP 和发送用例共同使用。
- `src-tauri/tests/、锁定的 lettre API`：新增本地 SMTP 假服务器或受控协议测试；根据实际库 API 判断可证明的发送阶段。

## 实施步骤

1. 移除 SMTP Plain 分支；Implicit TLS 与 STARTTLS 必须建立受保护连接才发送认证。
2. 按 SMTP 响应区分认证拒绝、4xx 和 5xx；只有有证据表明尚未被接受时才允许自动重试。
3. DATA 已发出但最终应答丢失、阶段无法证明或崩溃时返回结果不确定；不要根据一般网络错误猜测服务器未接受。

## 验收

- 实际发送路径遇到 plain 必须在建立连接前直接拒绝，断言零网络访问；入口配置校验不能代替协议发送路径的此项验收。

- 模拟 TLS 不可用、认证拒绝、4xx、5xx、DATA 后断线和最终 250；分别断言状态与重试决策。
- 日志、错误和测试输出不包含密码或令牌；Graph 发送结果使用同一业务语义。

完成并通过相关验证后，将本任务涉及的代码和测试单独提交；更新总览状态。

