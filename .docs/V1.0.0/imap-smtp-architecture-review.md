# IMAP / SMTP 与架构复核及修改建议

## 1. 复核基线与结论

- 复核日期：2026-11-16。
- 代码基线：`91e92a6c47620848e51dbf8067adf3ac7ffc4eec`；开始复核时工作区干净。
- 范围：IMAP 连接、认证、同步、解析、IDLE、远端变更；SMTP 构造与发送；账号、写信、同步用例；后台调度；相关模型、仓储、前端附件入口及现有测试。
- 方法：重新读取当前代码并检索调用点，核对协议规范与锁定版本 `imap 2.4.1` 的 API。未运行测试、未连接真实邮箱；验收场景均为待实现的验证要求，不表示已经通过。
- 本次仅新增本文档，没有修改业务代码。

结论：会话优先、前端 Port/Adapter、领域层无 IO、协议字段路由和数据库事务这些基础可以保留；当前主要缺口集中在远端身份、同步完整性、发送状态和后台任务保障。存在会影响远端数据和用户操作结果的确定性缺陷，应先修复这些行为，再收敛架构。

优先级：P1 表示可能误操作远端、漏信、丢失发送内容或阻断关键能力；P2 表示一致性、性能、可维护性或能力完整性问题。本文中的“风险”表示满足所述条件时可能发生，不等于已经在真实邮箱复现。

所有建议遵循根目录 [AGENTS.md](../../AGENTS.md)：按未上线 V1 直接修改正确模型；不保留旧 IPC、旧 schema 的兼容双轨；不引入 Repository/TransactionManager trait 对象；不新增 Rust `interfaces/` 层。

## 2. 相比上次分析，当前已经改善的部分

| 内容 | 最新代码证据 | 本次判断 |
|---|---|---|
| MIME 正文递归解析 | `infrastructure/imap/parse.rs::collect_text_parts / walk_parts` | 已解决“只遍历一层 multipart”的问题，不再列为未修复项；附件排除与 MIME 结构语义仍有缺口，见 I-12 |
| 正文预览 | `parse.rs::html_to_plain / truncate_preview` | 已优先使用 plain，否则去 HTML 标签、处理常见实体；不再认为 HTML 原文直接作为预览 |
| 日期回退 | `sync.rs` 获取 INTERNALDATE，`parse_rfc822` 接收 `internal_date_ms` | Date 缺失时已有回退；收件时间仍优先取 Date，语义问题见 I-12 |
| 删除扫描抬高游标 | `sync.rs` 已删除用全量远端 UID 推高 `last_uid` 的循环 | 该特定问题已修复；分页截断和单封失败跨越仍存在，见 I-02、I-05 |

上面的改善不代表 IMAP 同步已完整修复，不能直接把上次报告中的条目原样作为当前结论。

## 3. 当前问题与具体修改方式

以下源文件路径以仓库根目录为基准，Rust 路径统一省略 `src-tauri/src/`。以函数名定位，避免后续修改导致行号失效。

### I-01 / P1：单封删除回退为整夹 EXPUNGE

**证据**：`infrastructure/imap/client.rs::uid_delete` 先设置目标 UID 的 `\Deleted`，`uid_expunge` 发生任何错误后调用 `session.expunge()`。

**影响**：普通 EXPUNGE 会清除当前文件夹内所有带 `\Deleted` 的邮件，包含其他客户端先前标记的邮件。网络故障、权限错误也被当作回退理由，扩大了删除范围。协议行为见 [RFC 3501 §6.4.3](https://www.rfc-editor.org/rfc/rfc3501.html#section-6.4.3)。

**修改方式**：

1. 删除任何错误都回退普通 EXPUNGE 的逻辑，探测 UIDPLUS/MOVE 等能力后明确选择策略。
2. 若产品的“删除”指移入废纸篓，优先使用受支持的 UID MOVE，并明确 Trash 的发现和配置。
3. 永久删除仅在可限定 UID 的能力下执行；不具备能力时返回明确的能力错误或保留待处理状态，不清理其他 UID。
4. 已设置 `\Deleted` 后失败也必须保留操作进度；不得把它当作完全没有副作用的失败。

**验收**：A 已被另一客户端标为删除，用户删除 B；模拟 UID EXPUNGE 不支持、NO、断线，A 均不能被本次操作额外永久清除。

### I-02 / P1：最新 200 封截断后推进游标，遗漏其余邮件

**证据**：`infrastructure/imap/sync.rs::sync_folder` 对排序后的 UID 列表保留最新 `page_limit` 条；`application/use_cases/sync_use_case.rs::sync_imap_mail` 固定传入 200，并保存最大成功 UID。

**影响**：首次有 UID 1～1000 时只拉取 801～1000，下一轮从 1001 开始，1～800 没有后续补拉入口。离线新增超过 200 封也会遗漏。

**修改方式**：

- 为 V1 先实现最容易验证的有界升序批处理：选取当前未处理范围最早的一批，落库成功后继续下一批，每次执行有总时限，下一轮恢复。
- 若保留“最新优先”，必须分开保存新增邮件进度和历史补拉进度，例如 `incremental_high_water_uid`、`backfill_before_uid`、`backfill_complete`；首次最新批次不能等价于历史同步完成。
- 进度与该批邮件、聚合任务同事务提交。UI 区分“一轮执行完成”和“历史同步完成”。

**验收**：首次 1000 封、离线新增 450 封、分页中断重启、UID 不连续，最终应无遗漏；每次执行的下载量保持有界。

### I-03 / P1：没有新增时误报“无法读取正文”

**证据**：`sync_folder` 使用 `UID last_uid+1:*`，FETCH 遍历时跳过 `uid <= last_uid`，之后仍以原 `uid_list` 非空且 `fetched_ok == 0` 判断失败。

**影响**：非空邮箱没有新增时，该范围仍可能返回最后一封旧邮件；所有候选被跳过后误报错误，后续 FLAGS 和删除核对也不执行。该范围语义见 [RFC 3501 §6.4.8](https://www.rfc-editor.org/rfc/rfc3501.html#section-6.4.8)。

**修改方式**：在 SEARCH 结果进入分页和 FETCH 前，增量路径先过滤 `uid > last_uid`；对加一使用检查运算，避免 UID 上界溢出。有效新增集合为空应正常继续 FLAGS/删除核对。只针对实际请求但未取得内容的 UID 报获取失败。

**验收**：已同步最大 UID=100，服务器对 `UID 101:*` 返回 100；本轮成功，且远端已读变化、删除仍能落库。另测空邮箱和 UID 上界。

### I-04 / P1：后台下载正文会设置远端已读

**证据**：`sync_folder` 和 `fetch_one_rfc822` 均优先获取 `RFC822`；`BODY.PEEK[]` 只作为失败回退。

**影响**：同步动作可能把未读邮件标成已读。`RFC822` 等价于 `BODY[]`，区别于不设置 Seen 的 PEEK，见 [RFC 3501 §6.4.5](https://www.rfc-editor.org/rfc/rfc3501.html#section-6.4.5)。

**修改方式**：所有后台正文下载统一使用 `BODY.PEEK[]`，删除会设置 Seen 的兜底；如需降级，只降级可选获取字段。已读写入只经过显式用户操作与对应 mutation。

**验收**：同步前后服务器未读标记不变；批量和单封重试路径都验证。

### I-05 / P1：缺正文补抓失效，游标仍可能跨过失败邮件

**证据**：`sync_folder` 在检查 `item.body()` 之前调用 `seen_uids.insert(uid)`；后续单封重试遇到 seen UID 会跳过。只要其他 UID 成功，就可能继续提交更大的游标。

**影响**：批量响应包含 UID、但缺少正文时不会补抓；例如 UID 101 缺正文、102 成功，保存 102 后无法再通过新增查询补回 101。I-02 修复后该问题仍独立存在。

**修改方式**：区分已返回 UID、成功完成 UID 和待重试 UID；仅在解析成功后记完成。游标只能越过已经处理或确认远端消失的候选；如允许越过失败项，必须把失败 UID 同事务持久化为重试任务。对单封损坏内容设置独立失败状态，避免一个坏信无限阻塞整个邮箱。

**验收**：批量缺正文、单封补抓恢复；中间 UID 解析失败、后续 UID 成功；故障恢复后缺失邮件能补回。远端恰好删除该 UID 时可正确结束任务。

### I-06 / P1：IMAP 身份缺少 UIDVALIDITY，旧操作可能作用于新邮件

**证据**：`parse.rs::remote_id` 仅生成 `imap:{folder}:{uid}`；`sync_folder` 发现 UIDVALIDITY 变化只重新下载；`client.rs::uid_set_seen / uid_delete` SELECT 后不校验预期 UIDVALIDITY。

**影响**：邮箱重建后相同 UID 可能代表不同邮件；新内容会命中旧本地身份，旧 pending mutation 也可能修改或删除新邮件。旧 deleted 状态还可能阻止新一代同 UID 邮件入库。

**修改方式**：

1. 用类型和数据库唯一键明确表示 `(account_id, mailbox, uidvalidity, uid)`，不能只给字符串换一个通用名称。
2. UIDVALIDITY 变化后使旧位置与游标失效，终止或隔离引用旧一代的 mutation，再建立新一代投影；不要把“身份失效”直接解释为用户永久删除。
3. 每次远端写操作 SELECT 后校验预期 UIDVALIDITY，不匹配则拒绝写并请求重同步。
4. `internet_message_id` 只能辅助关联；不能假设全局唯一，也不能无条件用它合并所有邮件。

**验收**：旧代 UID 10 与新代 UID 10 内容不同；旧代待删除/待已读任务均不得作用于新代，旧内容不得覆盖新代。

### I-07 / P1：附件只有元数据，实际发送为空

**证据**：`src/features/compose/useComposeDraft.ts::addAttachments` 只提取 name/size/mime；`application/dto/compose_dto.rs::ComposeAttachmentDraft` 没有内容引用；`outbound_use_case.rs::send_via_smtp` 传 `attachments: Vec::new()`。Graph 外发模型同样未接上这些附件。

**影响**：用户看到附件列表，本地 `has_attachments` 为真，收件人实际收到的邮件没有附件。仅附件邮件甚至可能被发送成空正文邮件。

**修改方式**：先定义附件实体/引用和 ComposePort 的导入契约，再由 Adapter 将内容交给 Rust 暂存；Rust 返回稳定附件 ID。发送前在 application 校验所属账号/草稿、存在性和大小，读取持久化附件构建实际 MIME。消息、发送任务、附件引用同事务落库，文件导入采用暂存与清理流程。两种发送协议共用该内容来源。完成前至少拒绝带附件的实际发送，避免静默丢弃。

**验收**：同名附件、二进制附件、仅附件邮件、重启后发送、暂存文件缺失和发送失败重试；接收端文件字节与原始文件一致。

### I-08 / P1：发送错误被忽略，缺少持久化发送任务及结果状态

**证据**：`compose_use_case.rs::send` 提交消息、移除内存草稿后，以 `let _ = flush_pending_message(...).await` 忽略发送错误。检索当前 `src-tauri/src`，`flush_pending_message` 只有此处调用，未发现定时恢复 PendingLocal 的入口。`outbound_use_case.rs::flush_pending_message` 只在发送前读状态、成功后写 Synced，没有任务认领。

**影响**：失败无法可靠反馈，消息可能长期滞留；并发发送可能重复提交；SMTP 已接受后本地落库失败，后续再次发送会重复。返回的 bundle 也是发送前的 PendingLocal 快照。

**修改方式**：

- 在本地提交时同时创建持久化发送任务；由公共 worker 通过条件更新认领，记录 lease、尝试次数、下次执行时间和结构化错误。
- 明确区分排队、发送中、服务器已接受、失败和结果不确定；不要用同一个 Synced 概括所有过程。
- 在首次发送前生成并保存稳定 Message-ID，以及可重用的邮件内容/MIME；重试不重新生成身份。
- 已明确拒绝的永久失败停止自动重试；瞬时失败退避；最终 DATA 发送后连接中断等情况进入“结果不确定”，不能盲目无限重发。
- 发送任务的接受结果、消息状态和 UI ChangeSet 在本地事务中一致更新。草稿何时清空取决于明确的“已入队”契约，UI 不得将入队等同于成功投递。

**限制**：任务认领和稳定 Message-ID 能降低重复，但 SMTP 不提供普遍的幂等键，不能承诺严格恰好一次。DATA 最终应答超时的重复风险见 [RFC 5321 §4.5.3.2.6](https://www.rfc-editor.org/rfc/rfc5321.html#section-4.5.3.2.6)。

**验收**：离线发送、重启恢复、两个 worker 同时认领、认证失败、服务器接受后数据库失败、最终应答丢失；UI 分别显示真实状态且不静默丢失内容。

### I-09 / P1：XOAUTH2 被重复 Base64 编码，令牌续期未接入

**证据**：`client.rs::Xoauth2::process` 返回 Base64；锁定的 imap 2.4.1 在 `authenticate` 内部再次编码。`MailSecret` 保存 refresh_token/expires_at_ms，但通用 IMAP/SMTP 路径只读取 access_token。

**修改方式**：Authenticator 返回原始 SASL 字节/字符串，只让库编码一次；处理认证失败 challenge 时按协议结束交换。通用令牌获取封装为基础设施模块函数，使用到期时间、刷新授权配置和账号级并发保护；刷新失败区分可重试与须重新授权。先在 application 取得有效 token，再调用 IMAP/SMTP。不能仅凭 refresh_token 字符串就宣称支持任意 OAuth 服务，需完整授权服务器、客户端和 scope 配置。

**验收**：模拟服务器校验解码一次后的 SASL 数据；到期前刷新、并发刷新、refresh token 轮换、授权撤销；不得把 token 写进日志。依据：[imap 2.4.1 authenticate 源码](https://docs.rs/imap/2.4.1/src/imap/client.rs.html#478-480)。

### I-10 / P1：IMAP 变更队列缺少定时重试入口

**证据**：`message_use_case.rs` 为失败 mutation 记录下次执行时间；`runtime/mail_sync_scheduler.rs` 只筛选 Graph 账号后调用 `flush_due_mutations`；IMAP watcher 只调用邮件同步。用户动作会即时触发一次 flush，但不能替代恢复任务。

**修改方式**：将 mutation 与发送任务处理放入所有已登录账号共用的后台调度；接收同步、mutation、发送、聚合各自具备进度与错误处理，某项失败不阻断其他任务。mutation 增加认领/版本保护，避免即时任务与定时任务并发发送旧状态；账号退出后停止派发。

**验收**：IMAP 账号离线改已读/删除，首次失败后不再触发用户动作，恢复网络即可补推；重启恢复；连续更改同一封邮件时最后意图生效。

### I-11 / P2：SMTP 回复头、纯文本正文和服务器副本关联不完整

**证据**：`outbound_use_case.rs::send_via_smtp` 从 `message.internet_message_id` 构建 In-Reply-To/References，而 compose 已把父信 ID 存到 `message.in_reply_to`；HTML 邮件的 text/plain 部分仅取截断的 body_preview。发送结果只有 accepted，未保存本次生成的 Message-ID，也没有 IMAP Sent APPEND 路径。

**影响**：回复线程断开，纯文本阅读器看到截断正文；不自动保存 Sent 的服务器没有远端已发送副本；自动保存副本的服务器后续同步可能产生与本地消息未关联的第二条记录。

**修改方式**：发送自身稳定 Message-ID，In-Reply-To 使用父信字段；独立保存并构造 References 链；text/plain 从完整正文生成。明确已发送归档策略：根据账号/服务器行为配置自动保存或客户端 APPEND，保留原 MIME；APPEND 失败只重试归档，不重发 SMTP。关联服务器副本时使用账号、发送任务、Message-ID 等证据，避免无条件按 Message-ID 去重。

**验收**：新邮件与回复头正确；长正文 plain 不截断；自动保存与不保存 Sent 两类服务端均可归档；归档失败不造成邮件重复发送。

### I-12 / P2：递归 MIME 已改善，但仍未区分附件、草稿和时间语义

**证据**：`parse.rs::walk_parts` 递归取第一个 plain/html，没有判断 Content-Disposition；`parse_rfc822` 固定 `has_attachments=false`、`is_draft=false`、importance=Normal，且 received_at/sent_at 都取 `header_date.or(internal_date_ms)`。FETCH FLAGS 目前仅提取 Seen。

**影响**：text/html 附件可能被选成正文；附件存在状态不准确；选中 Drafts 时草稿可能进入普通会话；发送者 Date 会影响收件排序。合法地址组也被 `push_role` 的 Single-only 遍历忽略。

**修改方式**：保留当前递归，按 multipart/alternative、mixed、related 与 disposition 选择正文；独立收集附件元数据，定义 inline/CID 与下载引用。保留并映射 Draft 等必要 FLAGS。sent_at 使用邮件 Date，received_at 优先 INTERNALDATE，必要回退策略显式定义。地址组展开成员，避免丢失参与人。预览转换优先选用能正确处理实体和结构的实现，避免继续堆字符串替换。

**验收**：嵌套 mixed/alternative/related、HTML 文本附件、CID 图片、草稿、时区与 Date 缺失、合法地址组；检查正文选择、会话归属与列表排序。

### I-13 / P2：FLAGS 全量逐封往返，大邮箱同步成本高

**证据**：`sync_folder` 对每个 known UID 调一次 `uid_fetch(..., "(FLAGS)")`。`sync_account_blocking` 先收集所有文件夹结果再返回，任一文件夹失败会使之前尚未落库的结果一起丢弃。

**修改方式**：FLAGS 按有界 UID 集合批量 FETCH，按返回 UID 映射；普通 IMAP 保留批量核对兜底，有能力时增加 CONDSTORE/QRESYNC 优化。文件夹/批次逐个返回并由 application 独立提交进度，不必等待整个账号所有正文下载完成。错误按文件夹汇总，保留已经完成的进度。

**验收**：上万封已知邮件不产生逐封 FLAGS 网络往返；其中一个文件夹失败，其他已完成批次可恢复，内存与执行时间有界。

### I-14 / P2：IDLE 保活被当成超时，任务缺少生命周期管理

**证据**：`idle.rs::run_idle_until_cancel` 没有取消参数，调用 set_keepalive + wait_keepalive，不会返回 TimedOut；`mail_watcher.rs` 只 SELECT INBOX，无其他文件夹定时兜底，started 集合没有任务移除/取消机制，新增账号靠 300 秒扫描发现。

**影响**：仅非 INBOX 变化时可能长期不同步；任务无法在退出、修改协议/凭证时及时停止；已经建立的连接可能继续运行。方法实际语义见 [imap IDLE 文档](https://docs.rs/imap/2.4.1/imap/extensions/idle/struct.Handle.html)。

**修改方式**：用有限时等待 API，例如 wait_with_timeout，把超时转为可观测结果；短周期检查取消信号/配置版本。公共定时器始终覆盖勾选文件夹，IDLE 仅用于提前唤醒。用账号任务表管理 JoinHandle、取消信号和配置版本，账号事件触发创建/停止/重建，扫描仅作恢复兜底。

**验收**：INBOX 无变化但其他勾选夹有新信；退出账号、切换协议、替换凭证、服务端断线和不支持 IDLE；任务能在明确时间内退出或重建。

### I-15 / P2：连接超时与凭证输入处理不一致

**证据**：`client.rs::connect_tcp` 只尝试 DNS 第一个地址；STARTTLS 使用库连接入口，绕过自定义连接/读写超时；密码登录先 trim 密码。`trigger_mail_sync` 外层 timeout 包裹 spawn_blocking 的等待，但超时不终止已启动的阻塞任务。

**影响**：部分双栈网络无法回退；STARTTLS 可能长期阻塞；含首尾空格的合法密码改变；外层释放同步 gate 后旧阻塞任务还可能继续占用连接和执行 FETCH。

**修改方式**：统一 TLS/STARTTLS 的连接预算和读写超时，受总预算约束尝试地址；不要 trim 密码。显式处理 greeting 和握手状态，遵循锁定库 API。为阻塞循环增加取消检查与有界操作，gate 的释放与任务实际结束语义一致。使用阻塞 imap crate 本身不违反架构，无须仅为形式更换为异步库；若更换，也要先定义取消语义。

**验收**：第一个地址不可达、STARTTLS 不回应、TLS 握手停滞、总超时、取消、含空格密码；没有不断累积的后台阻塞任务。

### I-16 / P1：密码与令牌以明文 JSON 保存

**证据**：`infrastructure/mail/credentials.rs::save` 将完整 MailSecret 序列化后直接 `tokio::fs::write` 到 `*.mail-secret.json`。写入非原子覆盖；秘密类型还派生 Debug。

**修改方式**：使用操作系统凭据存储，或由系统保护密钥的加密存储；数据库仅保存凭据引用和必要的非秘密元数据。禁止完整秘密的 Debug 输出。若有文件式元数据，使用原子替换；退出时传播或记录凭据删除失败，避免静默保留有效密码。V1 可清理本地开发凭据并重新登录，不保留长期明文兼容读取分支。

**验收**：磁盘普通 JSON 不含密码/token；中断写入不破坏上一个有效版本；退出和删除失败可观测；日志不泄露秘密。

### I-17 / P2：账号更新先提交，后验证和保存秘密

**证据**：`mail_account_use_case.rs::upsert_imap_smtp_account` 在构造和校验 MailSecret 前提交数据库事务；password/access_token 缺失或秘密保存失败时，账号已被写入并标记登录。

**修改方式**：所有纯参数校验提前，包含端点、端口、认证必填值和 TLS 策略；凭据先写入新的独立引用，数据库事务最后切换账号配置和凭据引用。数据库失败时新引用作为未引用资源清理；旧引用在切换成功后回收。外部凭据存储与 SQLite 无法靠一个 SQL 事务实现原子提交，应明确补偿/恢复流程，避免覆盖旧凭据后无法回退。

**验收**：缺密码、凭据存储失败、数据库提交失败、修改已有账号失败；不能把无可用凭据的配置作为已成功登录账号使用。

### I-18 / P2：SMTP 所有发送错误都被标记为可重试

**证据**：`infrastructure/smtp/send.rs::send_message` 将 transport.send 的任何错误映射为 `Remote { retryable: true }`；同一实现支持 `MailTlsMode::Plain`，而 IMAP 明确禁用明文。

**修改方式**：根据 lettre 错误类别与 SMTP 状态区分认证、临时失败、永久拒绝和结果不确定，不让 5xx 或错误配置无限重试。明确 V1 传输安全策略，建议账号校验时统一拒绝普通远端账号的明文凭据传输；若确需本地开发 SMTP，显式限制为开发配置。不要只根据返回一个错误就断言服务器绝未接收，见 I-08。

**验收**：认证拒绝、收件人永久拒绝、4xx 暂时失败、TLS 不可用和 DATA 应答丢失；任务状态和 UI 提示与实际类别一致。

## 4. 架构问题及目标形态

### A-01：Graph 命名背后是身份模型与同步语义缺失

当前 Message 的 `graph_id/graph_change_key/created_at_graph`、通用 RemoteMessage 的 GraphImmutableId、IMAP 复用 GraphMessageChange 与 delta_link/next_link，使 IMAP 的 mailbox generation、分页和移动语义无处表达。

建议定义纯 Rust 的协议定位值对象与同步进度：Graph immutable ID；IMAP mailbox、UIDVALIDITY、UID；Graph delta/next 与 IMAP incremental/backfill 分别表达。数据库可用关联位置表保存协议投影，本地 MessageId/SessionId 继续作为稳定业务身份。Graph 账号和 IMAP 账号共用写入与聚合流程，但协议身份不能强行同形。

IMAP 跨夹移动可能表现为旧 UID 消失、新夹产生新 UID；文件夹只应影响远端位置。设计时还需覆盖多位置副本：不能把“某个位置消失”直接等同于整封业务邮件被删除，也不能仅靠相同 Message-ID 自动判定所有副本同一。V1 应明确关联证据和保守策略。

### A-02：基础设施反向依赖 application

证据包括 `infrastructure/imap/sync.rs`、`graph/mail.rs` 依赖 application 的 RemoteMessage/变更 DTO，多个 repository 依赖 application 的查询 DTO、任务类型和 PortError。与现有架构约定的依赖方向不符。

修改时区分类型归属：

- 业务实体、纯规则、远端身份等无 IO 语义进入 domain。
- 具体协议结果、存储结果与技术错误放在 infrastructure 自己的契约/类型模块；application 向上转换为用例 DTO 和 AppError。
- 纯分页/筛选值可放在合适的底层纯类型模块，不能为了共享把所有 IPC DTO 塞进 domain。
- repository 继续使用模块函数与 `&mut SqliteConnection`；跨仓储事务继续由 application 控制。

目标是让 application 依赖 infrastructure/domain，infrastructure 不再导入 application，而非增加一套 trait 注入体系。

### A-03：用例依赖运行时，形成双向关系

`sync_use_case` 依赖 runtime 的 MailSyncGate/ChangeDispatcher，dispatcher 内部持有 Tauri AppHandle；`compose_use_case` 直接接收 AppState；`message_use_case` 直接使用 Tauri async_runtime 启动后台任务。runtime 又调用这些用例。

建议 application 接收明确资源参数，返回业务结果与 ChangeSet；runtime/command 调用方负责 commit 成功后的分发、后台唤醒和任务生命周期。任务认领、gate 与重入策略由统一的调度入口执行，所有手动与定时触发共用入口。长流程按批次返回结果，避免“整个账号全部成功才发事件”，让已提交变更可见。

这样无需创建 Rust interfaces 层，也无需引入 Repository trait；用例仍可直接使用 SqlitePool 与具体事务。

### A-04：Graph 与 IMAP 拥有不同的后台可靠性保障

Graph scheduler 包含同步、mutation 与 locate；IMAP watcher 独立实现登录、端点组装、认证与调度，遗漏 mutation 恢复。发送任务又不属于任何周期处理器。

建议保留一套账号后台任务管理：收信同步、远端 mutation、发送、聚合和投影刷新共享生命周期、重试、观测与账号状态检查；各任务独立失败处理。Graph delta、IMAP IDLE 是收信驱动方式，协议特有 locate 仍按能力路由。IMAP 认证/端点解析提取为基础设施模块函数，不要在 sync、message、watcher 三处重复拼装。

### A-05：收发协议分开了，认证与配置有效性尚未分开

`domain/entities/mail_transport.rs::AccountMailTransport` 有独立 receive_protocol/send_protocol，但只有一份 auth_kind/username/secret，无法自然表达 IMAP 与 SMTP 使用不同认证。大量 Option 允许 IMAP 缺 host、SMTP 缺 port 等无效状态，错误延迟到运行时。

建议用 ReceiveConfig/SendConfig 枚举表达有效配置，各自引用 AuthConfig/CredentialRef；允许两者显式共用同一引用。输入 DTO 在 application 入口完成校验，领域类型构造后满足必填不变量。协议仍按配置选择，服务商品牌只做预填模板。

`repository/mail_transports.rs::ensure_default` 在配置缺失时直接写入 Graph 默认也应收敛：默认值只用于明确创建 Graph 账号，运行中的账号缺协议配置应暴露为配置错误。这样不会将损坏配置默默路由到 Graph。

### A-06：完整快照、局部变更和发送状态没有明确分型

FLAGS-only 通过“空 subject + body=None”制造 RemoteMessage 占位，再在 `sync_use_case::upsert_remote_message` 猜测更新类型。`RemoteMessagePatch/RemoteField` 虽然已定义，但未接入实际路径。发送状态又借用同一 MessageSyncStatus。

建议变更显式分型，例如完整快照、FlagsChanged、位置失效/移出信号；部分字段采用明确的 Unchanged/Value 语义，不使用业务字段的空值作标记。发送任务单独建状态机，与接收同步状态分离。按唯一实际用途接入或删除未使用的 Patch 类型，不保留两套形状。

## 5. 建议实施顺序与改动范围

| 阶段 | 工作 | 主要文件/模块 | 退出条件 |
|---|---|---|---|
| 1. 阻止数据与操作损失 | I-01、I-03、I-04；附件未接通前明确拒绝；发送结果可见 | imap/client、imap/sync、compose/outbound 用例、ComposePort/Adapter | 不扩大删除范围，不因同步改已读，无新增不报错，不静默丢附件/吞发送失败 |
| 2. 修正模型与任务契约 | I-06、I-08；A-01、A-05、A-06 | domain、schema、repository 模块契约、application DTO、前端 Port | 远端身份带 generation，收发配置有效，发送与局部变更显式建模 |
| 3. 完成同步与后台恢复 | I-02、I-05、I-10、I-13～I-15；A-03、A-04 | IMAP 同步、公共调度、mutation/发送任务仓储 | 分页不漏、故障可恢复、账号生命周期可取消、阻塞任务有界 |
| 4. 完成通用收发能力 | I-07、I-09、I-11、I-12、I-16～I-18 | 附件存储、MIME、OAuth/凭据、账号用例、SMTP | 附件可收发，回复/归档闭环，认证与错误分类正确 |
| 5. 收敛依赖与文档 | A-02，其余跨层整理 | infrastructure 类型、application、runtime、相关设计文档 | infra 不依赖 application，用例不依赖 Tauri/AppState，契约只有一套 |

阶段可分小步提交；模型相关修复必须沿“领域规则 → Repository 函数契约 → application/infra → IPC/前端 Port 与 Adapter”推进。不要为了赶阶段在永久代码里加入旧字段别名或兼容读取链。需要重置开发库时写清重建和重新登录步骤即可。

## 6. 验证策略与完成标准

现有 `src-tauri/tests/mail_protocol_routing.rs` 主要验证协议配置存取与游标编解码；`repository_transactions.rs` 验证本地仓储事务。当前未发现覆盖上述 IMAP/SMTP 交互行为的完整模拟服务端测试，不能用配置测试通过代替协议正确性。

建议按行为补充有意义的验证：

1. **纯规则测试**：UID 候选过滤/批次进度、UIDVALIDITY 不匹配、MIME 正文选择与附件排除、回复头、错误分类。
2. **模拟协议测试**：无新增时范围返回旧 UID、缺正文、FETCH 过程中删除、UID EXPUNGE 失败、XOAUTH2 编码、IDLE 超时/取消、SMTP 拒绝及结果不确定。以协议响应驱动断言，不依赖生产邮箱。
3. **SQLite 集成测试**：邮件与游标/重试任务同事务提交、发送认领并发、过期 lease 恢复、旧 generation mutation 失效、账号更新失败补偿、远端移出后的会话投影刷新。
4. **受控端到端验证**：有/无 IDLE、有/无 UIDPLUS/MOVE、TLS/STARTTLS、不同 Sent 自动保存行为、附件和中文 MIME。使用测试账号；任何破坏性操作均限定测试数据。

最终检查：会话仍为业务核心，Message 归属唯一 Session；文件夹只表示同步位置；协议流量只在 Rust infrastructure；Graph 继续使用 graph-rs-sdk；UI 通过 Port/Adapter；跨仓储事务由 application 控制；无旧版本兼容双轨。

本文是静态复核与修改计划，不是测试通过报告。实施后应逐项记录实际验证结果、提交版本和剩余限制。
