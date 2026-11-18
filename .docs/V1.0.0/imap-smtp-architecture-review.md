# IMAP / SMTP 与架构待解决问题

## 1. 当前复核结果

- 更新日期：2026-11-17。
- 复核基线：`61b16ab46eb2052797fcf03ad3555a374fad214a`，证据以本轮最后读取的当前源码为准；本次只修改本文档。
- 原 18 个实现问题：**4 个原始缺陷已解决并删除正文，2 个部分解决，11 个未解决，1 个 V1 暂缓（I-09）**。暂缓不代表已修复。
- 本轮新增 **I-19 / P1**：HTML 邮件默认允许远程资源，内容隔离与 CSP 保护不足。静态代码已确认配置与执行路径，未验证实际跟踪请求或 XSS/IPC 利用。
- 当前 V1 实现问题共 **14 个：12 个未解决、2 个部分解决；P1 为 7 个，P2 为 7 个**。另列 1 个 V1 暂缓实现项；其“显式拒绝通用 OAuth2 配置”要求仍由 A-05/I-17 纳入 V1，不随完整功能一起暂缓。
- 原 6 个架构问题：均未完整解决，继续保留；它们与实现项有关联，不重复计入上面的实现问题 P1/P2 数量。
- 下文仅列尚需处理的内容，编号沿用原文，因此存在跳号。部分修复项只描述剩余问题，不再沿用过时证据。
- 本次补充修订保留以上问题数量、优先级及状态；新增的是待实现的规则和验收，不表示缺陷已修复。重点核对了 Message/RemoteMessage、mutation 仓储及执行、文件夹发现/同步、SMTP 发送和 ChangeDispatcher；跨条目规则统一见 §3“统一核心模型的不变量与操作决策表”。
- 本轮重新读取相关源码、调用点和测试定义；只修改本文档，没有修改业务代码，没有重新运行测试或连接真实邮箱。保留此前复核记录的 17 项 Rust 测试通过结论，该历史记录不等于本次重新验证，也不等于所有协议行为已经实测。

QQ 邮箱历史范围受限已经确认与账号“仅收取最近 30 天”设置有关，不把这一配置现象作为代码缺陷。当前代码使用 `UID SEARCH ALL` 与本地已下载 UID 求差集，最新优先每轮最多下载 200 封，每次 FETCH 最多 8 封，未完成历史主动续跑；不再以 high/low 游标排除下载候选。这是当前正确性基线，并非永久唯一算法；允许在发现进度、实际持久化和失败重试分离后，安全引入高水位增量发现。UID 不保证数字连续。

**统一模型与 V1 范围**：Postlet 以标准 IMAP/RFC 邮件能力为业务基线，所有协议共用本地 MessageId、SessionId、单一可选 RemoteLocation 和文件夹模型。Message 固定归属一个 Account、一个 Session，最多一个有效远端位置；不同账号/不同位置的实体独立，同位置重复同步幂等，不做内容去重。Graph 独有的 Immutable ID、conversationId、changeKey 不参与身份、受控移动关联、会话归属或其他业务正确性决策。Rust infrastructure 适配远端访问、操作定位和同步，输出统一事实；application 可按账号协议选择具体模块函数，但不解析 remote_key。前端 Port/Adapter 仍负责桌面入站。通用 IMAP/SMTP V1 仅支持密码、应用密码/邮箱授权码；不建设通用 OAuth2/XOAUTH2 生命周期，已实现的 Graph OAuth 保留。

**V1 已认可的策略**：只对 INBOX 建立 IDLE，所有勾选文件夹由约 60 秒的等待超时/轮询兜底同步。这个取舍本身不是问题，不要求为每个文件夹建立长连接。IDLE 已有进入监听、变化、超时与失败日志；日志中的“本批下载”不包含单纯 FLAGS 更新数量。

优先级：P1 为远端误操作、内容丢失或关键能力不可用；P2 为一致性、恢复、性能及能力完整性。风险条目均说明触发条件，不表示已经在真实邮箱发生。

遵循 [AGENTS.md](../../AGENTS.md)：按未上线 V1 修改正确形态，不保留兼容双轨；domain 无 IO；Repository/用例继续使用模块函数和具体事务；不增加 Repository/TransactionManager trait 对象或 Rust `interfaces/` 层；前端遵循 Port → Adapter。

下文 Rust 路径省略 `src-tauri/src/`；函数名用于稳定定位。

## 2. 实现问题

### I-01 / P1：单封删除可能扩大为整夹永久清理

**状态**：未修复。

**问题与影响**：删除目标 B 时，可能顺带永久清除同一文件夹中其他客户端已标记 `\Deleted` 的 A。

**原因与依据**：`infrastructure/imap/client.rs::uid_delete` 给目标设置 Deleted 后，`uid_expunge` 任何错误均回退为普通 `session.expunge()`。普通 EXPUNGE 清理的是整夹所有 Deleted 邮件，网络错误也会触发扩大范围。

**解决方案**：取消普通 EXPUNGE 回退。先探测 UIDPLUS/MOVE 等能力，再根据产品语义选择指定 UID 永久删除，或移动至明确配置/发现的 Trash。不具备安全能力时返回能力错误或保留待处理状态。设置 Deleted 后失败必须记录已发生的副作用，不将其当成完全未执行。

**验收**：预置其他 Deleted 邮件；模拟不支持 UID EXPUNGE、NO 和断线，本次删除均不得额外清除其他邮件。

### I-06 / P1：IMAP 身份与待推操作缺少 UIDVALIDITY

**状态**：未修复；同步检测 generation 变化并重新下载，并未解决身份和写操作问题。

**问题与影响**：邮箱重建后相同 UID 代表另一封邮件，可能命中旧记录，或让旧待删除/待已读操作作用于新邮件。

**原因与依据**：`imap/parse.rs::remote_id` 仍为 `imap:{folder}:{uid}`；`sync_folder` 仅按 generation 决定是否信任 known 集合；`uid_set_seen/uid_delete` SELECT 后不校验预期 UIDVALIDITY。`sync_use_case::upsert_remote_message` 仍按同一个 graph_id 查找，且会保留旧 session 或拒绝复活旧 DeletedRemote 记录。

**解决方案**：采用 A-01 的统一远端定位字段，**一个 Message 最多一个有效 RemoteLocation**，位置唯一键为 `(account_id, folder_id, remote_key)`。同位置重复同步更新同一 Message；不同位置创建独立 Message。所有移动均按源位置移除、目标位置新增处理，不替换原 Message 的位置或保留其 MessageId。IMAP Adapter 将 UIDVALIDITY + UID 编码为不透明 remote_key；Graph Adapter 使用 API 操作必需的普通消息 ID。业务层不解析标识，不依赖 Graph 专有身份能力。

IMAP 文件夹同步状态另存 UIDVALIDITY。generation 变化时失效该夹旧代定位、游标及引用旧定位的待执行操作；旧 Message 保留为无有效位置，新代邮件按新位置创建 Message，不能用新代相同 UID 继承旧身份、已读意图或删除任务。Adapter 必须在执行写操作的同一选夹连接上 SELECT 校验预期 generation，不匹配则拒绝并请求同步。Graph/IMAP、Postlet 发起或外部发起的移动都按源位置移除、目标位置新增处理；即使 remote_key 字符串未变，folder_id 改变也属于不同位置。RFC Message-ID、In-Reply-To、References 仅用于关系/会话判断，不用于跨位置合并实体。

**验收**：旧代与新代 UID 10 的 remote_key、MessageId 均不同，旧任务不能改动新代，旧删除标记不能阻止新信入库。Graph 普通 ID 改变、旧 ID 不可定位时不能把旧任务重定向到其他消息。两种协议同位置重复导入均幂等；位置消失不直接永久删除本地 Message。所有移动均使源实体失去有效位置，目标按其位置幂等建立独立 Message；复制保留源实体并在目标建立独立 Message。业务层不解析 remote_key，不读取 Graph 专有身份属性。

### I-07 / P1：附件只有元数据，发送时被静默丢弃

**状态**：未修复。

**问题与影响**：用户选了附件，消息显示 has_attachments，但收件人收不到；仅附件邮件可能发成空邮件。

**原因与依据**：`src/features/compose/useComposeDraft.ts::addAttachments` 仅保存 name/size/mime；`application/dto/compose_dto.rs::ComposeAttachmentDraft` 无内容引用；`outbound_use_case::send_via_smtp` 固定传 `attachments: Vec::new()`。Graph 外发模型也未接通这些附件。

**一致性风险**：附件由 SQLite 元数据/稳定引用和文件系统实际内容两部分组成，两者不共享事务。先写文件后 SQL 提交失败会留下孤儿文件；先提交引用后写文件失败会留下无效引用。前端临时 File/路径不能支撑重启和发送重试；共享内容若随一封消息删除可能误删其他消息仍在引用的文件。这些是附件存储实现必须覆盖的失败路径，当前尚无完整内容存储流程，不将其描述为已复现的数据丢失。

**解决方案**：

1. 先定义统一附件实体、稳定引用和 ComposePort 导入契约，由 Rust 管理内容，Graph/IMAP/SMTP 共用同一存储生命周期，不依赖前端临时路径。
2. 内容先进入临时区域，校验归属、长度/完整性等后，在同一受管理文件系统内原子移动到最终位置；原子移动不等于跨 SQLite 原子事务或崩溃持久性，需明确平台持久化保障。
3. SQLite 事务内一起保存消息、附件引用和发送任务；仅当内容已持久化、可读取且引用提交成功，任务才允许实际发送。每次重试前重新核验可用性，缺失则失败并提示，绝不退化成不带附件发送。
4. SQL 失败时清理未引用内容；崩溃重启核对暂存、孤儿文件和失效引用。并发导入/发送与回收要协调，删除前检查所有持久化引用及运行中任务，不能只靠文件名或一封消息的删除动作回收。
5. 明确草稿删除、消息删除、发送失败、账号退出、取消和崩溃恢复的保留/回收时机；退出不应无条件删除仍被消息或重试任务引用的内容。接通前明确拒绝实际发送含附件的邮件。

**验收**：二进制/同名/仅附件及接收端字节一致；注入临时写失败、移动失败、SQL 回滚、引用提交后内容缺失、发送中断和回收竞态。在每个边界模拟退出/崩溃并重启，重试内容仍可用、孤儿可回收、无效引用可见；多消息共享附件时删除其中一封不得破坏其他引用，不得静默发送不完整内容。

### I-08 / P1：发送错误被忽略，没有可恢复的发送任务

**状态**：未修复。

**问题与影响**：失败后 UI 仍可能按成功流程清空草稿，邮件长留 PendingLocal；并发或远端接受后本地写失败可能导致重复发送。

**原因与依据**：`compose_use_case::send` 先提交消息并清除内存草稿，再 `let _ = flush_pending_message(...).await`。全局检索仍只有此处调用，未发现定时恢复 PendingLocal 的入口。`outbound_use_case::flush_pending_message` 无认领/lease，成功后才写 Synced，返回到 UI 的 bundle 仍是发送前状态。

**解决方案**：消息与持久化发送任务同事务入队；worker 条件认领，记录 lease、尝试次数、下次时间、结构化错误。区分排队、发送中、服务器接受、失败、结果不确定；首次发送前保存稳定 Message-ID 和内容，重试复用。永久拒绝停止自动重试，瞬时失败退避，DATA 最终应答丢失不盲目重发。UI 明确区分“已入队”和“发送成功”，提交后分发准确状态。

**边界**：SMTP 没有普遍可用的幂等键；稳定 Message-ID 与任务认领不能保证严格恰好一次。

服务器接受只表示接收了投递责任，不等于最终到达收件箱；V1 不建设通用送达回执状态机，UI 使用“服务器已接受”语义，不声称最终送达。发送任务在网络调用前持久化“发送中/尝试标识”；已开始传输而没有确定拒绝/接受证据的崩溃恢复进入“结果不确定”，不能只因 lease 到期重发。接受证据应尽快保存；若保存失败，内存仍在时只重试本地确认，崩溃后无法证明接受则保持不确定，提示用户核对。Sent 副本只能按 §3 规则辅助核对，不把“暂未找到”当作未发送证明。A-06 定义发送、归档和同步的独立状态。

**验收**：离线、重启恢复、并发认领、认证拒绝、服务器接受后数据库失败、最终应答丢失；内容不丢，状态准确。

### I-10 / P1：IMAP 已读/删除失败后没有周期重试

**状态**：未修复；网页已读能够拉回本地，不代表本地操作失败能够补推。

**问题与影响**：离线更改已读或删除，第一次推送失败后可能长期不再执行。未完成的本地已读意图还会在拉取时优先于远端 FLAGS，造成状态长期不一致。

**原因与依据**：`message_use_case::spawn_flush_due_mutations` 只在用户动作后触发一次；失败记录 available_at，但周期性的 `flush_due_mutations` 只出现在筛选 Graph 账号的 `runtime/mail_sync_scheduler.rs`。IMAP watcher 仍只调用收信同步。`sync_use_case::upsert_remote_message` 优先 pending.desired_is_read。

**解决方案**：将 mutation 恢复纳入所有已登录账号的公共调度，与收信、发送分开失败处理。增加认领/版本保护，避免即时与定时任务并发覆盖用户最新意图；按错误类别退避或停止，失败可见，退出后不派发。

业务意图与远端执行目标必须分开持久化：每项 mutation 保存 AccountId、MessageId、意图版本，以及入队时唯一位置的 folder_id、remote_key、预期 generation/定位修订号、目标值和执行阶段；不得到执行时才读取 Message 的“当前 folder/id”重新选目标。没有有效位置时不得派发远端已读/删除，保留本地操作或返回可见的不可操作结果。generation 失效、位置消失、连续反向操作、远端成功后本地失败的共同规则见 §3 决策表。当前 `mutations::acknowledge/record_failure` 已按 version 条件更新，但这只保护本地确认，不能阻止旧网络请求晚到覆盖远端新值；仍需按 Message 串行、派发前检查版本及完成后补推最新值。

**验收**：IMAP 离线改已读/删除，恢复网络且无新用户动作也会补推；覆盖重启与连续反向更改场景。

增加目标安全验收：入队后移动/删除位置、UIDVALIDITY 改变或文件夹角色改变，旧任务不得改目标；已读 true→false 的旧请求晚到不得确认/清除新版本，最终远端收敛到 false。远端成功而本地确认回滚后重启，按 §3 的操作类别恢复，不重复执行未知结果的移动/复制。

### I-11 / P2：SMTP 回复头、纯文本和已发送副本缺少闭环

**状态**：未修复。

**问题与影响**：回复线程可能断开；纯文本阅读器只看到截断预览；不自动保存 Sent 的服务端没有副本，自动保存的服务端后续同步又可能出现重复本地记录。

**原因与依据**：`outbound_use_case::send_via_smtp` 从本信 internet_message_id 取 In-Reply-To/References，而 compose 把父信 ID 存在 in_reply_to。text/plain 取 body_preview。`smtp/send.rs` 结果只有 accepted，本次生成的 Message-ID 未保存；没有 Sent APPEND/关联流程。

**解决方案**：持久化本信稳定 Message-ID；正确使用父信 in_reply_to，单独构造 References 链；plain 来自完整正文。明确服务器自动保存或客户端 APPEND 的账号策略，复用同一 MIME；归档失败只重试归档，不重新 SMTP 发送。Sent 副本按其位置幂等入库，不与其他位置的 Message 合并，不按 Message-ID 去重。

发送接受、投递结果、Sent 归档和位置同步分别记状态，完整契约见 A-06。账号归档策略明确为“服务器保存”或“客户端 APPEND”；未知时要求配置确认，不同时盲用两种策略。客户端 APPEND 仅在服务器接受已确认后执行，使用首次发送前持久化的归档 MIME；保留相同正文/附件/Message-ID，SMTP envelope 与 Bcc 隐私处理另存，不因重试重新生成 Date、boundary 或内容。APPENDUID 可在 Adapter 内映射为确定目标位置；未返回映射或最终应答丢失时先核对 Sent，不能盲目再次 APPEND。服务器保存模式不因短期未同步到副本就自动 APPEND，超时显示归档待核对。

发送任务与 Sent Message 的关联只是任务确认/展示关系，不是跨位置去重：有本客户端 APPEND/Graph 操作返回的确定位置时，可将**尚无位置**的本地发送 Message 绑定该位置；若该位置已被同步建成另一个 Message，则保留既有实体，任务引用它，原无位置发送记录退出普通会话展示，不合并两个 Message。没有确定位置映射的自动保存副本按独立 Message 入库；已接受的本地发送记录仅在发送状态区保留，普通会话以远端 Sent Message 为准，尚未同步副本时显示“等待已发送副本”。不为隐藏重复按 Message-ID/正文猜关联。归档任务无确定证据时保持待核对，不删除任何副本，也不阻塞 Sent 消息的正常同步展示。

**验收**：新信/回复头正确，长正文 plain 完整，两种 Sent 行为可归档，归档失败不重发。Sent 同位置重复同步幂等，不同位置的副本始终为不同 Message；发送给另一个已登录账号时 Sent 与 INBOX 独立。缺失/重复 Message-ID、正文相同或仅回复关系均不合并实体；无位置发送记录与远端 Sent 的展示规则不产生两条普通会话记录。

增加 SMTP 已接受但 APPEND 被拒/断线、APPEND 成功但本地回滚、服务器自动副本延迟/重复、相同内容多次发送及服务器改写正文的验收；归档恢复不得触发 SMTP，证据不全保持待核对。

### I-12 / P2：MIME、特殊文件夹角色与日期语义不完整

**状态**：未修复这些剩余问题；递归解析和基本预览已有，不再要求重做。

**问题与影响**：文本/HTML 附件可能被误选为正文；附件状态不准确；草稿可能进入普通会话；收件排序受发送者 Date 影响；地址组成员遗漏。

**原因与依据**：`imap/parse.rs::walk_parts` 递归取首个 plain/html，没有 Content-Disposition 和 multipart 结构选择；parse_rfc822 固定 has_attachments/is_draft=false、importance=Normal，received_at/sent_at 都为 header_date.or(internal_date_ms)。FLAGS 仅提取 Seen，push_role 仅处理 Single 地址。`imap/client.rs::discover_folder_tree` 只保存名称/父路径/delimiter，未保留 LIST 的 SPECIAL-USE 属性；`domain/entities/sync_state.rs::MailFolderSyncState` 无统一角色字段。Graph 侧已有 INBOX well-known 标注，但未形成完整角色模型；现有文件夹名称、默认勾选规则也不能替代角色的可靠识别。

**解决方案**：

- 保留现有递归解析，按 alternative/mixed/related 与 disposition 选择正文，独立收集附件和 inline/CID 引用；映射必要邮件 FLAGS；sent_at 使用 Date，received_at 优先 INTERNALDATE 并定义回退；展开地址组。预览转换使用成熟的 HTML 结构/实体处理。
- 在统一文件夹模型中表达 `Inbox / Sent / Drafts / Trash / Junk / Archive / All / Custom`，不同协议 Adapter 转换成相同角色，不建立两套业务目录。
- IMAP 优先使用服务器提供的 [RFC 6154 SPECIAL-USE 属性](https://www.rfc-editor.org/rfc/rfc6154.html#section-2)，如 `\Sent`、`\Drafts`、`\Trash`、`\Junk`、`\Archive`、`\All`。INBOX 有自身协议语义，不是 RFC 6154 新增的统一 `\Inbox` 属性；Custom 表示无已确定特殊角色。标准并不要求服务器用同一机制返回上述所有业务角色。
- 识别顺序为服务器角色优先；缺失时允许用户手动指定；谨慎名称匹配只能作为待确认默认建议。保存来源，明确角色冲突/服务器信息变化后的确认策略。
- 区分邮件 FLAG `\Draft` 与文件夹属性 `\Drafts`，不能只凭其中之一或目录名称替代全部业务判断。统一定义各角色的同步、归档和展示规则：Drafts/Trash/Junk 不误混入普通会话，Sent 参与外发展示，Archive/All 按下述范围处理。文件夹仍不是默认主导航。

**重复展示策略（V1）**：每个 Message 只按自身当前文件夹角色决定范围。INBOX、Archive、Sent、Custom 的非草稿消息进入普通会话；Drafts、Trash、Junk 及带 Draft/Deleted 标志的消息进入相应范围。All 是可能重叠的汇总范围，已同步的 All Message 保留为独立实体，但不进入默认普通会话和未读计数；仅显式查看 All 范围时展示，并提示仅同步 All 不会填充普通会话。INBOX 与 Archive 同时存在相同内容时允许两条独立记录及独立未读计数，标注账号/文件夹来源，不按内容折叠或联动操作。这是展示过滤，不是底层去重，也不自动改变用户勾选的同步文件夹。角色变更只重算受影响消息/会话投影、校验旧任务，不改变身份；Sent 角色不单独证明作者身份或发送成功，外发方向仍按账号自身地址及可信发送任务判定。

**验收**：嵌套 MIME、HTML 附件、CID、日期和地址组；服务器返回特殊属性、无 SPECIAL-USE、非英文/重命名目录、手动映射、歧义名称、角色冲突均可预测。分别测试 Draft FLAG 与 Drafts 属性：草稿、已发送、垃圾、删除目录识别正确，不错误混入普通会话；Graph/IMAP 同角色规则一致。INBOX/Archive/All 三个位置建立三个独立 Message，默认普通会话仅展示前两个，All 范围展示 All 实体；修改任一实体不改其他实体。仅勾选 All 的结果与提示符合上述契约。

### I-13 / P2：同步提交和失败隔离不足；性能优化按需推进

**状态**：部分修复。FLAGS 已按最多 200 个 UID 批量核对，逐封往返缺陷不再保留；剩余如下。

**问题与影响**：一个文件夹失败可丢弃本轮其他夹尚未提交的下载结果；永久损坏邮件可能反复占据候选，整页均失败时阻断更旧历史。当前全量 UID/FLAGS 核对有随邮箱增长的开销，但是否构成实际性能瓶颈尚待基准测试，不能仅因使用全量算法就要求 V1 立即重构。

**原因与依据**：`imap/session_ops.rs::sync_account_blocking` 将所有文件夹结果收集后一次返回，循环中使用 `?`；application 收到整个 batch 后才逐夹提交。`sync_folder` 每轮 SEARCH ALL 并核对所有已知 FLAGS；missing_imap_page 每轮取最新的缺失项，没有逐 UID 的失败记录/退避。如果最新候选一整页都永久失败，较旧项无法进入下载页；FETCH 网络错误也会中断整个调用。

**解决方案与状态契约**：

1. 正确性改进优先：application 按夹/批次提交、汇总失败；单 UID 记录尝试、退避及可见错误，让其他候选继续，失败不能当作完成。
2. 分开保存三种事实：**远端 UID 发现进度**、**正文等所需内容实际下载并持久化完成状态**、**失败/待重试 UID**。进度、候选与失败记录须有明确的事务/恢复边界，不能只保存一个“成功最大 UID”代表所有事实。
3. 首次同步/历史补齐继续使用 UID 集合差集、失败记录和主动续跑，防止中间缺口被遗漏。当前 `UID SEARCH ALL` + 差集保留为正确性基线，但不是永久唯一方案。
4. 日常增量可在 UIDVALIDITY 不变时，由 IMAP Adapter 使用已发现最大 UID 或 UIDNEXT 等优化新邮件发现。UID 数字不连续，发现不代表下载成功；只有发现结果已可靠保存为候选/重试状态后才能推进对应发现进度。高水位不能让失败 UID 永久失去补抓机会。
5. 增量发现不能替代 FLAGS 更新、远端删除检测、历史补漏和 generation 检查；保留周期集合一致性核对及失败补偿。generation 改变时旧发现进度和旧操作失效，按 I-06 恢复。
6. 大邮箱优化以请求数、耗时、内存和服务端行为测试为依据；安全高水位、CONDSTORE/QRESYNC、核对频率等是可选演进，不要求 V1 立即全部实现，更不能以速度换漏信风险。

**验收**：一个文件夹失败不抹掉其他已完成批次；最新整页坏信时仍能下载更旧有效邮件。未来增量优化需覆盖稀疏 UID、UIDNEXT 与实际候选有差异、发现后崩溃/下载失败、离线大量新增、旧信 FLAGS 变化/删除、周期补漏及 UIDVALIDITY 改变；最终与集合基线一致。先记录大邮箱性能数据再决定优化，不把尚未运行的性能测试写成既定故障。

### I-14 / P2：监听任务缺少退出、取消和配置变更生命周期

**状态**：部分修复。IDLE 已用 wait_with_timeout，等待约 60 秒后同步所有勾选夹；历史主动续跑、5 秒发现账号、监听日志均已有。仅保留生命周期问题。

**问题与影响**：退出、切换协议、清理数据或替换凭据时，没有明确的任务终止与重建；已有连接可能继续存活，长期失败任务不能被统一回收。

**原因与依据**：`runtime/mail_watcher.rs` 用永久 started 集合记账号，任务没有 JoinHandle/取消信号；watch_one_account 无限循环。run_cycle 会检查协议、重读凭据，但不会退出并从任务表移除。run_idle_until_cancel 名称含 cancel，实际没有取消参数；退出后也未在每轮明确检查 is_signed_in。

**解决方案**：账号任务表持有 handle、取消信号和配置版本；退出/配置切换触发取消和重建，定期扫描只作兜底。对阻塞协议循环设置取消检查和有限等待，终止后移除任务表记录。保留 INBOX IDLE + 全部勾选夹定时同步的 V1 策略。

**验收**：退出、切换协议、替换凭据、清理本地数据、断线后任务有明确状态和退出期限；重新登录不重复创建或遗留旧任务。

### I-15 / P2：连接超时、地址回退和密码处理不一致

**状态**：未修复；IDLE 的有限等待不等于整个阻塞同步任务可取消。

**问题与影响**：双栈地址首个不可达时失败；STARTTLS 可能长期阻塞；合法密码被改变；外层超时后旧任务仍可能运行，与下一次同步重叠。

**原因与依据**：`imap/client.rs::connect_tcp` 只取 DNS 第一个地址；STARTTLS 绕过自定义 connect/read/write timeout；登录 trim 密码。`sync_use_case::trigger_mail_sync` 的总 timeout 只能取消等待，不能终止运行中的 spawn_blocking，guard 却会释放。

**解决方案**：统一 TLS/STARTTLS 连接与 IO 总预算，受预算约束地尝试地址；密码原样传递。按锁定库 API 处理握手/greeting；阻塞循环可取消且单步有界，gate 释放与实际任务结束一致。不因使用阻塞 crate 本身就要求换库。

**验收**：首地址不可达、STARTTLS/握手停滞、总超时、取消、含空格密码；不积累重叠阻塞任务。

### I-16 / P1：凭证明文保存且非原子覆盖

**状态**：未修复。

**问题与影响**：本地普通 JSON 包含密码/access/refresh token；中断覆盖可能破坏凭据；Debug 或删除失败处理可能暴露/残留秘密。

**原因与依据**：`mail/credentials.rs::save` 直接 serde_json + tokio::fs::write，MailSecret 派生 Debug。`account_use_case::sign_out` 忽略通用凭据删除结果。

**解决方案**：使用系统凭据存储，或由系统保护密钥的加密存储，数据库保存引用；秘密类型去除或脱敏 Debug。文件式元数据原子替换。退出时删除失败可见且可恢复。V1 允许清理开发凭据后重新登录，不保留明文兼容读取分支。

**验收**：普通磁盘 JSON 与日志无秘密，写中断保留有效状态，退出/删除失败可观察。

### I-17 / P2：账号配置先提交，凭据后校验/保存

**状态**：未修复。

**问题与影响**：缺密码、缺 token 或秘密存储失败后，数据库账号/协议配置已改变并可能显示已登录。

**原因与依据**：`mail_account_use_case::upsert_imap_smtp_account` 在构造 MailSecret 和检查必填值之前 tx.commit，再调用 credentials::save。

**解决方案**：纯参数校验提前，包含端点、端口、认证必填和 TLS 策略。新凭据写入独立引用，数据库事务最后切换引用及有效账号状态；失败清理未引用新凭据，切换成功后回收旧引用。SQLite 与外部凭据存储不共享事务，需补偿/恢复，不能覆盖旧秘密后假装可以回滚。

**V1 认证边界**：通用邮箱入口必须在任何账号/凭据写入前显式拒绝 OAuth2/XOAUTH2，覆盖 IPC 直接调用与后台路径，不能只依赖当前 UI 固定 password。已有通用配置若选了 OAuth2，应明确提示当前不支持，不进入错误认证或静默降级。Graph OAuth 的账号、令牌流程及共用枚举中的合法 Graph 分支必须保留；完整通用 OAuth2 规划移至 §6。

**验收**：缺凭据、秘密存储失败、SQL 提交失败、更新已有账号失败均不留下不可用却已启用的配置。构造通用 OAuth2 IPC 请求得到明确不支持错误且无存储/网络副作用；Graph OAuth 回归不受影响。

### I-18 / P2：SMTP 错误不分类型，明文策略不一致

**状态**：未修复。

**问题与影响**：认证/永久拒绝被当作可重试，无法正确指导重试或用户修正；明文模式是否允许没有一致边界。

**原因与依据**：`smtp/send.rs::send_message` 对 transport.send 的任意错误映射 retryable=true；SMTP 支持 Tls::None，IMAP 禁用 Plain。

**解决方案**：按 lettre 错误和 SMTP 状态区分认证、暂时失败、永久拒绝、结果不确定。普通远端账号统一要求受保护连接；如需本地开发 SMTP，限定为明确开发配置。错误不必然意味着远端未接受，结果不确定应与 I-08 联动。

**验收**：认证拒绝、4xx、5xx、TLS 不可用、DATA 应答丢失分别得到正确状态和重试策略。

### I-19 / P1：HTML 邮件默认允许远程资源，渲染隔离与 CSP 保护不足

**状态**：新增，静态确认存在默认远程资源允许路径及防御缺口；未运行恶意邮件、浏览器网络捕获或 IPC 利用测试。不能写成“HTML 完全未经清洗”或“已经证实可执行脚本/调用 IPC”。

**已有保护与代码证据**：

- `src/features/conversations/MailReadingPane.vue` 将正文交给 `src/shared/mail-content/MailBodyView.vue`；聊天预览使用 Vue 文本插值，没有发现该路径直接 v-html 输出原始正文。
- MailBodyView 使用 `vue-letter 0.2.3`。本轮读取实际安装的 `node_modules/vue-letter/dist/vue-letter.js`：内置 lettersanitizer 使用标签/属性白名单，删除 script/iframe 等元素及不允许属性，限制 href/src 和 CSS URL 协议，处理样式，并为链接设置 noopener/noreferrer。存在清洗链路，不把危险 script/事件属性直接放行当作已确认事实；清洗是否可被特定畸形输入绕过尚待测试，不能称为“完善保护已证明”。
- MailBodyView 的 click capture 拦截非 http/https 外链（mailto 单独放行）；`src/adapters/tauri/createTauriPorts.ts::createTauriPorts` 的平台适配实现再次只允许 http/https 经 opener 打开。主窗口 capability 没有配置 remote URL 授权。这些是已有边界，不能描述为任意外链都可调用高权限。

**已确认的问题、原因与影响**：

1. MailBodyView 配置 allowed-schemas 为 http/https/mailto，没有设置 rewriteExternalResources 或“加载远程内容”授权。所检查依赖会保留允许的图片 src 和 CSS url，因此外部图片/跟踪资源默认允许自动请求，缺少隐私授权和统一资源策略。实际网络请求需在目标 WebView 中捕获确认。
2. MailBodyView 明确 `use-iframe=false`，清洗结果经依赖 innerHTML 进入主应用 DOM，没有独立的低权限邮件渲染上下文。`src-tauri/tauri.conf.json` 的 CSP 为 null；`capabilities/default.json` 将 core/opener/allow-ipc 授予 main，`permissions/app-ipc.toml` 含写信、删除等命令。配置缺少纵深保护：**若**清洗发生绕过，内容可能落在有应用权限的页面；尚未证明绕过或成功 IPC 调用，不能据此宣称已复现 XSS/命令执行。
3. 当前没有受控 CID → 内嵌附件资源映射；cid 不在允许协议中，不能通过直接放开任意 asset/file/data 协议来补齐。没有发现完整的 HTML 附件预览入口，其权限风险待实现方案与测试核实，不作为已确认可利用漏洞。

**解决方案**：

- 复用并评估成熟 Sanitizer，锁定明确的标签、属性、CSS 与 URL 策略，补维护/升级与回归验证；不自行开发 HTML 解析器或渲染引擎。外部正文、HTML 附件统一视为不可信。
- 默认阻止外部图片及其他资源自动加载，用户主动授权后按邮件/来源范围加载；覆盖 HTML 与 CSS 资源路径，必要时避免清洗阶段自身发起请求，不能只拦截链接点击。
- CID 只映射本邮件拥有的、经过校验的受管理内容；限制资源和协议，防止 file、任意本地 asset、脚本协议及未受控 data 内容获得访问。
- 采用经验证的低权限、隔离渲染方案（例如正确配置 sandbox 的独立邮件上下文），不给邮件内容主窗口 IPC 能力。不能把 useIframe=true 当成自动安全：当前依赖 iframe 分支本身未设置 sandbox，需明确 origin、权限、跨上下文消息和链接处理。
- 配置适当的 CSP、Capabilities 与外链策略形成多层保护；开发/生产都验证。HTML 附件使用同等隔离或交给明确的外部应用处理，不能作为应用同源高权限页面加载。相关边界参考 [Tauri Capabilities](https://v2.tauri.app/security/capabilities/) 与 [CSP](https://v2.tauri.app/security/csp/)。

**验收**：用 script、事件属性、危险/编码 URL、SVG/MathML、畸形 HTML/CSS 等安全样例验证清洗；默认打开邮件/清洗时无外部跟踪请求，授权仅开放允许资源，拒绝仍生效。覆盖 CSS 资源、CID 越权、mailto/http 外链、file/asset/data、HTML 附件及新窗口。隔离内容不能访问主窗口或成功调用删除/发送等 IPC，检查 CSP 违例和 capabilities 实际执行。该组测试目前均未运行，不能以静态审查替代。

## 3. 架构问题

### A-01：通用邮件身份仍套用 Graph 模型

**问题**：缺少以标准 IMAP/RFC 能力为基线的本地身份、单一远端定位和生命周期规则。本地消息与远端操作 ID 混杂；不能将 Graph 额外标识的稳定性作为业务前提，也不需要跨账号/跨文件夹内容去重系统。

**原因与依据**：Message/RemoteMessage 当前仍有 GraphImmutableId、graph_change_key、created_at_graph；IMAP 使用 GraphMessageChange，进度编码在 delta_link/next_link 字符串里。这些是现有代码事实，不是要求未来继续使用相应 Graph 身份能力。虽然差集补拉已可工作，统一位置约束仍缺失，尤其见 I-06；本文不据字段存在就断言 conversationId 已参与实际聚合，但明确禁止未来以它或 changeKey 等决定业务身份和会话。

**解决方案：Message 最多一个远端位置，直接保存定位字段**：

```text
Account 1 ─── N Message
Session 1 ─── N Message
Message 1 ─── 0..1 RemoteLocation
```

所有协议统一按位置新增、更新、移除消息。**移动 = 源位置移除 + 目标位置新增**，Postlet 自己发起的移动也遵循同一规则：源 Message 保留为无有效位置的记录，目标使用独立 MessageId；源、目标不做实体关联或合并，Session 各自按既有规则确定。远端仍采用协议原生安全移动能力，本地模型不要求改写远端操作方式，也不要求为保留 MessageId 提供操作映射或并发合并设计。

**推荐存储**：沿用当前 Message 行承载定位的方式，改为协议中性的 `folder_id + remote_key` 可空字段组，并保存定位有效性、定位修订号和该位置的 FLAGS 观测；不新建位置集合表或 Message↔位置关联表。两字段同时为空表示没有定位，同时有值表示保存了一份定位；保存旧定位供幂等核对时须标明 missing/invalidated，不能当作有效操作目标。有效定位最多一份，`(account_id, folder_id, remote_key)` 对所有非空定位唯一，失效旧定位也占用其原键，防止同位置反复建新实体。无位置的发送记录不受这个唯一键限制。domain 可用可选 RemoteLocation 值对象表达同一字段组，不增加协议专用业务实体。

| 标识 | 唯一职责及不变量 |
|---|---|
| MessageId | Postlet 本地生成的唯一 ID；Message 固定属于一个 Account 和该账号内一个 Session，位置或内容不生成该 ID |
| RFC Message-ID | 标准邮件头，只作引用、关系和既有会话规则的输入；不加唯一约束，不用于合并实体 |
| RemoteKey | 仅定位远端邮件；IMAP Adapter 编码 UIDVALIDITY + UID，Graph Adapter 使用普通消息 ID；业务层不解析，也不当跨文件夹稳定身份 |
| AccountId | 所有权边界，Message、Folder、Session 和任务目标必须同账号；不同账号实体永远独立 |
| FolderId | 本地文件夹身份，映射远端文件夹名称/路径/ID；不是角色，不直接用远端路径替代本地身份 |

不同位置的邮件即使主题、发件人、正文、附件、RFC Message-ID 全部相同，也分别创建 Message。Session 聚合按既有纯领域规则独立执行，可以将多个 Message 放到同一会话展示，不能因此合并实体或联动操作；跨账号总览只组合账号内 Session/Message 查询结果，不共享 Message，也不新增跨账号 Session 身份系统。例如 A 向 B 发信且两账号均登录：A/Sent 的 M1 与 B/INBOX 的 M2 永远独立，已读、删除、发送/同步状态和任务失败互不继承。

**本轮补核源码与结构影响**：`domain/entities/message.rs::Message` 已有 account_id/session_id 与单一 folder_id，适合直接修订；`migrations/001_init.sql` 当前是 `(account_id, graph_id)` 唯一键，无 RemoteLocation 表。后续替换为上述三列唯一约束，移除 graph_change_key/created_at_graph 等统一模型中的 Graph 专名，标准 Date/INTERNALDATE 语义按 I-12；Message→Session 复合外键沿用，Message→Folder 加同账号约束。`imap/parse.rs` 直接用邮箱路径构造 FolderId，`graph/mail.rs::map_remote_message` 直接用远端 parentFolderId 构造 FolderId，`repository/folder_states.rs::upsert_discovered` 按该值继承游标/勾选：需改为本地 FolderId 与远端绑定分开保存，连同 mutation 固定目标一起改。V1 可重建开发库，不保留旧 Graph 命名或双轨模型；本次不执行迁移。

Graph Delta 可留在 Adapter 内作为发现变更/恢复同步的优化，输出统一位置/FLAGS/内容变更事实。Immutable ID、conversationId、changeKey 不参与任何身份、移动关联、会话归属或其他业务正确性决策；普通消息 ID 只用于操作定位。不新增 GraphLocation/ImapLocation 业务分支。

**验收**：

- 同账号同位置重复/并发同步只生成一个 Message；不同位置、不同账号均生成不同 Message，所有标准头和内容完全相同也不合并。
- Postlet/外部移动、Graph/IMAP 均按源位置移除、目标位置新增处理，目标实体不继承源 MessageId；目标已同步入库时仅按位置更新。复制保留源并在目标建立独立 Message。generation 变化时旧任务不能触及新代。
- A/Sent 与 B/INBOX 两实体独立修改/失败恢复；同 Session 内两个相同内容 Message 的操作也互不影响。缺失/畸形/重复 Message-ID 和仅有回复关系均不改变位置幂等规则。
- 不提供/改变 Graph 专有身份字段或移除 Delta 优化，不改变身份、会话与操作安全决策。以上均为待实现验收，不声称当前代码已满足。

### 统一核心模型的不变量与操作决策表

本节共同约束 A-01、I-06、I-10、I-12、A-06，不新增问题编号。规则均为本次确定的 V1 目标；数据库改动集中在 Message、Folder 和任务字段，不引入通用身份匹配系统。

**状态、文件夹和生命周期**：

| 归属 | 不变量、执行/失败规则 | 现有结构影响与最小验收 |
|---|---|---|
| Message 的唯一定位 | 定位有效性为 present/missing/invalidated；连接失败、关闭同步或一次不完整扫描不等于位置消失。只有当前代的完整集合核对或确定移出/删除事实才可标 missing；generation 改变标 invalidated。保留旧键只用于核对，不可写远端 | Message 增定位状态/修订号；完整扫描缺失与网络失败分别验收，旧键重复同步不得重复建实体 |
| 远端 FLAGS 与 UI 状态 | 当前唯一位置保存观测 FLAGS；UI 已读默认取该位置 Seen，本地最新有效已读意图覆盖；本地草稿状态独立，远端 Draft FLAG 与 Drafts 角色按 I-12 决定草稿范围。Deleted FLAG、Trash 角色、用户删除意图和远端位置消失分开记录，不能互相当成功证明 | 可保留 is_read/is_draft 为可重算缓存，另存远端观测及意图；两份同内容不同位置 Seen 不同时各自展示，不做聚合/传播 |
| 待完成意图 | 同事务保存本地展示意图、固定目标任务与 Session 投影；同步继续保存真实远端观测，但不覆盖最新有效意图。成功确认或用户取消后重算；终止失败保留可见错误并重算真实状态，不伪装远端已完成 | 改 `upsert_remote_message` 的 pending 覆盖与 DeletedRemote 短路；验收同步旧 FLAGS 不抹掉排队意图，失败后错误/真实状态可见 |
| 无有效位置的 Message | 远端消失/失效只停用定位和旧目标任务，保留内容、附件引用、Session 归属、错误记录；不自动物理删除。普通远端会话列表/未读计数排除无有效位置记录，本地草稿/发送状态由专门状态展示；账号清理或显式本地清除才回收，附件按 I-07 引用规则 | 拆除“DeletedRemote=用户删除/永久销毁”的混用；最后/唯一位置消失后内容可核对，重启不错误复活，附件不被提前清理 |
| Folder 身份 | Folder 固定属一个 Account；本地 FolderId 与远端绑定、名称、角色、UIDVALIDITY 分离。已确认本客户端重命名可更新原绑定，必须核对重命名后 generation；外部名称/路径变化无可靠连续性时旧夹退役、新建 FolderId，不按名称/角色猜身份 | Folder/同步状态增加绑定及退役/配置修订；验收重命名、同名删除重建均不继承错误 UID/游标/任务 |
| 目录删除/重建与角色变化 | 目录确定删除使其位置失效、任务停止；重建使用新 FolderId，不继承勾选/游标或待执行任务。角色变更只重算展示，不改变身份；语义任务保存源/目标 FolderId 与配置修订，执行前角色/绑定改变则停止并要求重新确认，不能把旧 Trash 操作转到新角色目录 | 不再由 `upsert_discovered` 按路径无条件继承；验收 Trash 改成 Archive 后旧删除任务不再派发，其他账号同名目录不受影响 |

**身份与操作决策**：

| 场景 | 必须采取的 V1 决策 | 失败/歧义处理及最小验收 |
|---|---|---|
| 同 RemoteLocation 重复同步 | 按三列唯一键更新原 Message，FLAGS-only 不改内容/Session；数据库唯一约束保证并发幂等。同代 missing 位置被权威核对重新确认存在，可恢复原实体；invalidated 旧代不能复用，新代是新键 | 内容空标题/空正文合法；并发重复输入只有一个实体；位置恢复不能继承已终止的删除任务 |
| 移动（Postlet 或外部发起） | 本地统一按源位置移除、目标位置新增处理：源 Message 标 missing 并停止其旧目标任务，目标按三列唯一键创建/更新独立 Message，按既有会话规则确定 Session；不继承源 MessageId，不建立源→目标身份映射。Postlet 发起时仅持久化安全执行所需的固定源位置、预期 generation/定位修订、目标 FolderId 和执行阶段 | 远端继续使用 IMAP UID MOVE、Graph 原生 move 等协议能力；本地拆成位置增删不等于远端拆成不安全的 COPY/DELETE/EXPUNGE。缺少安全移动能力则报能力错误或保留待处理，不扩大操作范围。结果不确定先核对、不盲目重发；源缺失不能单独证明目标已到达。验收本客户端/外部移动结果一致、目标先被同步发现或重复返回均同位置幂等、无跨位置实体合并 |
| Postlet 复制 | 源 Message 不变，新位置总是新 Message；COPYUID/Graph 返回值只用于确定复制结果和幂等入库，不建立共享实体 | 复制结果不确定先核对，不盲目重做 COPY；验收复制两条独立状态/任务 |
| 同账号不同夹、相同 Message-ID/全文 | 不同位置独立 Message，按 I-12 展示范围过滤 | 无跨位置匹配/自动合并后台任务；验收主题/发件人/正文/ID 全同仍独立 |
| Message-ID 缺失、重复、畸形；只有 In-Reply-To/References | 不影响按位置入库；合法标准头仅参与既有会话规则，回复关系不等于实体同一性 | 验收异常头不阻止新位置入库，不生成唯一键冲突 |
| 同信发给多个已登录账号 | 每个账号各自创建 Message，独立 Session 归属及已读/删除/重试 | A→B 示例中修改 M1 不改 M2，即使 UI 总览同时展示 |
| UIDVALIDITY 变化后重新发现 | 失效旧定位和旧任务；新代按新 remote_key 建新 Message | 新旧同 UID/同全文也不继承任务；I-06 的写前检查不可省略 |
| 已读/删除 | 用户选定 Message，任务固定其当前唯一位置；已读使用设置到目标值而非 toggle。默认删除为移动到已确认 Trash，本地同样按源位置移除、Trash 位置新增处理；永久删除只允许明确选定当前位置并遵守 I-01，不做整夹 EXPUNGE | 不作用于同内容其他实体；无安全删除能力则可见失败，不扩大范围。Trash 中永久删除仍须明确操作 |

**PendingMutation 的最小安全与恢复规则**：

1. 入队事务保存业务意图版本及唯一固定定位快照，目标同时校验账号归属、位置有效性、定位修订、文件夹绑定/配置修订和预期 generation。Adapter 在实际写入连接上校验 generation；业务层不解析 remote_key。当前 mutation 的 `(account_id, message_id)` 行可继续承载最新意图，不引入多目标子任务框架，但须补固定目标/阶段/认领字段。
2. 同 Message 的网络写串行；认领和派发前检查最新版本。反向已读更新使旧版本过期，旧请求若已发出无法撤销，完成后只记录该尝试结果，不确认新版本，继续按最新目标值补推。确认/失败写入必须以版本和 lease 条件更新；过期认领不能与仍在执行的旧连接并行写。验收 true→false、断线重启、旧响应晚到最终收敛为 false。
3. generation/定位失效或位置消失即停止旧目标；不通过 Message-ID、全文、同 UID、同名目录自动重新定位。移动后的目标是独立 Message，源实体旧读/删任务不转移，必要时由用户对目标实体重新操作。删除安全优先于自动补推。验收旧删除永不误伤新代、移动目标或复制件。
4. 同步请求携带本地观测序号/定位修订；写操作成功前发起的旧读取不能在成功后覆盖确认值，移动前的旧快照也不能恢复已移出的源位置。过期批次丢弃后重新核对；目标位置无论先由操作返回还是同步发现，都走同一位置唯一键入库，不需要跨位置关联、实体合并或专用并发合并流程。成功后同事务保存已知结果、确认对应意图及投影，再进行新一轮权威核对；远端其他客户端后来修改则正常反映，不能永久锁定本地意图。该序号不是 Graph changeKey。
5. 远端已成功而本地确认失败：进程仍在时只重试本地确认；崩溃后已读 set 可在目标仍有效时幂等重放。永久删除核对原位置已不存在可确认效果；MOVE/COPY/APPEND 最终结果未知先核对，无法确定则标“结果不确定”供人工处理，不重做可能产生新副本的动作。不能因旧位置消失就宣称目标 Trash/归档已成功。最小验收覆盖远端成功后 SQL 回滚、lease 恢复、断线后源缺失但目标未知。

这些规则只增加固定目标、版本及必要执行阶段，不建立跨位置身份推断或通用任务框架。发送/归档独立恢复见 I-08/I-11/A-06，提交与前端恢复见 A-03。

### A-02：infrastructure 反向依赖 application

**问题**：基础设施与应用 DTO/错误形成反向依赖。

**原因与依据**：imap/graph 依赖 application::dto::remote_models；repository 多处依赖 application 的查询、分页、任务类型和 PortError，违背约定依赖方向。

**解决方案：按职责定归属，避免机械搬移 DTO**：

| 类型/职责 | 归属及边界 |
|---|---|
| 统一 Message、RemoteLocation、Session、文件夹角色、领域不变量/纯业务规则 | domain；无 IO、不依赖 application/infrastructure/runtime/Tauri，不携带 Graph/IMAP 专用业务概念 |
| Graph/IMAP/SMTP SDK 数据、UID/服务器响应和协议编码 | infrastructure 的协议 Adapter 内部，进入用例前映射统一业务类型 |
| SQLx Row、数据库错误、查询构造等技术类型 | infrastructure repository；不泄露为 UI 业务契约 |
| 用例命令、编排结果、应用错误和查询用例输出 | application；不能因某个 repository 使用就全部搬至 infrastructure |
| Tauri 参数/序列化及展示传输 DTO | commands/入站边界按需映射；可复用纯用例 DTO，但不将所有 IPC DTO 塞进 domain |
| 真正跨层的纯分页、筛选或结果数据契约 | 先判断业务语义，能归已有统一业务/查询类型则复用；只有确有跨层稳定用途时放入小型无 IO 的纯契约模块，不借此增加通用接口框架 |

本轮实际依赖：repository::sessions/groups 直接使用 application 的列表/分页 DTO；多个协议模块返回 application::remote_models，技术错误 PortError 也在 application；application 同时调用这些 infrastructure 模块。domain 当前未发现实际反向导入，不把它写成已发生的领域 IO 污染。

允许依赖为 `commands → application → domain / infrastructure`、`infrastructure → domain`，runtime/组合根调用 application 并组织基础设施；如提取纯契约模块，各层单向依赖它，模块本身不反向依赖任何调用方。消除 `infrastructure → application → infrastructure` 与 A-03 的运行时环，不为了消除单个 import 增加 interfaces 或 trait。

具体处理应结合查询责任：repository 返回统一实体/必要的纯查询行，用例组装列表 DTO；确有共享语义的分页契约再提取。Repository 继续使用具体模块函数，application 可使用 SqlitePool 显式控制事务，repository 借用同一事务连接。目标是职责清晰和业务协议解耦，不是把所有 application DTO 下沉。

**修订时机**：与 A-01/A-06 同阶段确定类型归属和函数签名，同时修改调用方和仓储契约；不能先按反向依赖实现完新模型，最后再大规模搬移。统一位置/FLAGS/关联事实/操作结果及纯状态转换归 domain，SDK 响应和 SQL Row 留 infrastructure；ChangeSet/用例输出留 application，repository 不为返回 ChangeSet 反向导入 application。application 在同一事务内组合仓储写入、投影和变更版本。此处只梳理现有边界，不引入 Repository trait、TransactionManager、事件总线或 interfaces。

**验收**：infrastructure 不导入 application；跨仓储事务仍由 application 控制。

### A-03：application 依赖 runtime 和全局 AppState

**问题**：运行时调用用例，用例反向依赖运行时与 Tauri 状态。

**原因与依据**：sync/aggregate/account 依赖 runtime 的 gate/dispatcher，dispatcher 持有 AppHandle；compose/account 直接使用 AppState；message 用例直接 tauri::async_runtime::spawn。

**解决方案**：用例显式接收所需资源、返回结果和 ChangeSet；外部调度负责 gate、任务启动、取消、commit 后分发。长流程按提交批次产生变更，避免一个后续失败让前面已提交内容无法通知。无需增设 interfaces 或 Repository trait。

**提交与恢复契约（推荐 V1）**：

- ChangeSet 只描述**已成功提交**的变更。事务内可收集候选摘要，但 commit 失败必须丢弃，不能先发事件再提交。涉及位置、Message、Session 投影与 mutation 的写入由 application 在同一事务完成；网络调用不持有长数据库事务。
- 每个账号持久化单调 `change_version`，所有影响前端查询的提交（含任务状态/清理）在同事务递增。提交后 ChangeSet 带该版本；账号内版本与 IMAP generation、mutation version 严格区分。账号清空时保留/递增版本；账号删除后通过账号列表重查失效，重新创建使用新 AccountId。
- 前端事件只是刷新提示。启动、重新订阅、恢复焦点及低频兜底检查经 Port 查询账号版本；版本改变/跳号即重查受影响范围，无法恢复范围时重查该账号。列表/详情查询返回与数据同一读快照的版本；刷新期间收到更高版本继续刷新，不把旧查询结果标成最新。进程重启前的版本仍有效，不能用当前进程 AtomicU64 充当持久版本。
- 数据库提交成功但进程在通知前退出时，下次查询按持久版本恢复 UI，不回滚已提交事实，也不重新执行协议动作以“补事件”。无需持久化全部 UI ChangeSet 历史。
- **系统通知 V1 采用尽力而为**：仅 IncrementalSync 真正新增入站允许通知；允许提交与通知之间崩溃导致漏提示，重启不补放历史提醒，通知失败不影响收信状态。当前通知函数是 stub，并非已实现可靠投递。V1 不要求可靠系统通知，因此不新增 Outbox。若将来明确要求崩溃后仍补通知，才在产生通知资格的同一事务写局部 notification outbox，由外部 worker 发送/确认并按账号、消息、通知种类去重；OS 显示后本地确认前崩溃仍可能重复，不能宣称恰好一次，也不引入通用事件总线。
- 多夹/多批次采用每批提交并交付结果的用例边界，由外部循环调度/分发；后续错误单独汇总，不能只返回 Err 丢掉先前已提交摘要。每批都有独立版本。若在交付前崩溃或事件发送失败，UI 按版本补查；系统通知遵守上述尽力而为边界。不能把多批候选预先合成“已提交”ChangeSet。

**源码/数据库影响**：当前 `dto/change_set.rs::MailChangedEvent.generation` 和 `runtime/change_dispatcher.rs` 的 AtomicU64 仅进程内单调，emit 失败只日志；`TauriChangeEventsAdapter.ts` 也仅按该值过滤，不能证明崩溃恢复。新增持久账号版本、查询/事件版本契约及 Port/Adapter 刷新入口，移出用例中的 dispatcher/spawn；不要求对旧 DTO 兼容。

**验收**：用例不依赖 Tauri/AppState/runtime 具体分发器；手动和后台触发共享一致重入规则。

增加 commit 前失败不发事件、commit 后分发前强杀、前端漏事件/重订阅/焦点恢复、刷新请求乱序和两批成功第三批失败验收：数据库与查询版本一致、前两批仍可刷新，未提交批不出现；重启不重复协议动作、不补发历史系统通知。通知失败不回滚收信；若以后启用 outbox，另验同事务回滚、重复认领、OS 成功后确认失败，不借此扩大本次 V1 范围。

### A-04：后台恢复保障按协议割裂

**问题**：收信、mutation、发送的恢复能力不一致，重复组装认证与端点。

**原因与依据**：Graph scheduler 带 mutation/locate；IMAP watcher 仅负责收信和 IDLE；发送任务没有周期处理。IMAP 历史续跑已修复，但不能替代 I-08/I-10 的恢复职责。

**解决方案**：共用账号任务生命周期和恢复调度，收信、mutation、发送、聚合分别失败处理；Graph Delta 留在 Adapter 内作为同步优化，IMAP IDLE 用于变化唤醒，两者均驱动统一业务变更与后续处理。远端 locate 仅核对已知定位或受控操作结果，不通过 Graph 普通 ID/专有属性追踪跨文件夹身份；外部移出按 A-01 建立独立消息。端点和认证解析提取基础设施模块函数，不按邮箱品牌复制业务。

**验收**：不同协议下相同操作都具备离线重试、重启恢复、退出停止与观测能力。

### A-05：收发认证绑定，配置允许无效状态

**问题**：IMAP/SMTP 不能独立表达认证，缺端点等错误留到执行时才发现；缺协议配置还会静默默认 Graph。

**原因与依据**：AccountMailTransport 独立 receive/send_protocol，却仅一份 auth_kind/username，端点字段大量 Option；repository::mail_transports::ensure_default 在缺行时写 graph_default。

**解决方案**：ReceiveConfig/SendConfig 枚举各自关联 AuthConfig/CredentialRef，可显式共用引用。入口校验后类型满足必填不变量。Graph 默认只用于明确创建 Graph 账号，运行中的配置缺失应报错。品牌仅做预填，不做协议判断。

V1 的有效认证配置还必须区分范围：通用 IMAP/SMTP 仅密码或邮箱授权码；Graph 保留 OAuth。当前 AddAccountDialog 通用提交固定 password，但 IPC DTO 和 mail_account_use_case 仍接受 OAuth2，因此 UI 没有选项并不等于后端已显式禁止。不引入通用 OAuth 授权端点/scopes/token 生命周期；先按 I-17 在入口拒绝并在执行路径明确报不支持，保留 Graph 合法认证分支。

**验收**：不同收发用户名/凭据可用；无效组合不能构造或落库；缺配置不被悄悄路由 Graph；通用 OAuth2 被明确拒绝、无副作用，Graph OAuth 回归通过。此处不要求 V1 支持更多认证协议。

### A-06：局部变更靠空值占位，发送状态与同步混用

**问题**：完整快照与局部更新无显式区分，发送阶段含义不明确。

**原因与依据**：imap::flag_only_remote 构造空 subject/body=None 的 RemoteMessage；upsert_remote_message 据此猜测 FLAGS-only。RemoteField/RemoteMessagePatch 虽已定义但未接入；MessageSyncStatus 同时承担发送状态。

**解决方案**：显式变更类型表达完整快照、FlagsChanged、位置移出/失效；稀疏更新区分 Unchanged/Value。FLAGS 更新指向 Message 当前唯一定位和预期 generation/定位修订，不以空标题/空正文推测；更新该位置远端观测后，结合最新本地意图重算 UI 状态，不影响其他 Message。未返回某 FLAG 是 Unchanged；显式完整 FLAGS 集不含某 FLAG 才表示清除；无法获得的能力表达未知，不能伪造 false。接入或删除未使用 Patch 类型，保留一套实际契约，与 A-01/A-02 一起调整类型和仓储。

**状态分离**：

移动只产生源位置移出和目标位置新增两类事实，不增加“保持身份的移动”变更类型；源实体状态及任务不迁移到目标实体。原生操作返回值可确认执行结果、提供目标位置供普通入库，但不参与跨位置身份关联；未知结果按既有失败恢复规则核对，不自动重做移动。

| 状态归属 | V1 推荐状态与边界 |
|---|---|
| 发送任务（Message 关联） | 排队、发送中、服务器已接受、可重试失败、永久失败、结果不确定、取消；持久化尝试阶段/lease/错误。只有确定未接受的可重试失败自动重发；发送中 lease 过期但无法证明未提交则不确定。接受不是最终送达，不以 Synced 表示接受 |
| Sent 归档任务（接受后独立推进） | 等待服务器副本/待 APPEND、归档中、已确认归档位置、可重试失败、结果不确定、需配置；服务器保存或客户端 APPEND 为互斥账号策略。有确定操作结果才引用对应 Sent Message，不按内容匹配；APPEND 的未知结果先核对，归档任何失败不得重新入发送队列 |
| Message 的单一 RemoteLocation / 文件夹同步 | Message 行保存定位 present/missing/invalidated、该位置 FLAGS 观测及内容完整性；文件夹保存同步进度/错误。Sent 副本未同步不表示 SMTP 失败，位置存在也不证明发送任务接受 |
| 本地 Message / 展示投影 | 内容、账号/会话归属、本地草稿或无有效位置的保留记录；is_read/is_draft/展示范围等按 §3 派生，待完成意图单独覆盖。不同位置实体无共享状态，删除意图不是 DeletedRemote 同义词 |

首次发送前在内容持久化后，同事务保存发送任务、稳定 Message-ID、Date、完整可重放 MIME 引用、envelope 和附件引用；提交后才允许网络发送。编辑排队内容需取消尚未派发任务并创建新的内容版本；发送中/结果不确定时不直接改原尝试内容。SMTP/Graph Adapter 输出统一接受/拒绝/不确定事实，不把 Graph 普通操作 ID 当业务身份或最终送达证明。Graph 当前 `save_to_sent_items=true` 仅表示请求保存，未证明归档已确认；统一接入时遵循 I-11 的确定操作结果及独立位置入库规则。

**源码/数据库影响**：拆除 MessageSyncStatus 同时表示发送与删除的职责，增加发送/归档任务及不可变内容引用、位置同步状态和本地意图；`smtp/send.rs` 当前现场构造 MIME、仅返回 accepted，`outbound_use_case` 随后将 Message 标为 Synced，不能作为上述状态机已实现的证据。

**验收**：空标题/空正文合法邮件不会误判变更类型，局部更新不清空其他字段，发送阶段可区分。

增加相互独立验收：已接受但归档失败、归档已有关联但同步暂失败、最终应答丢失/发送中崩溃、发送前 MIME 持久化失败；重启后状态不混用，同一尝试内容保持稳定，未确认未接受的任务不自动重发。

## 4. 后续实施顺序

| 顺序 | 内容 | 对应问题 |
|---|---|---|
| 1. 安全止损 | 阻止扩大删除、附件静默丢弃、发送错误冒充成功、凭据泄露及 HTML 远程资源自动加载 | I-01、I-07、I-08、I-16、I-19（已静态确认路径，利用与网络效果待测试） |
| 2. 统一核心模型 | 共同确定 Message 0..1 定位、账号内身份、位置唯一键、Folder 身份、FLAGS/意图/发送归档状态及依赖归属；同时修订仓储函数/事务契约和相关表；账号/凭据配置一致，显式拒绝不支持的认证 | A-01、A-02、A-06；I-06、I-10、I-12 的模型部分；A-05、I-17；A-03 的版本/提交契约 |
| 3. 可靠收发与后台恢复 | 完成发送/归档任务边界、mutation 固定目标恢复、附件生命周期、单 UID 失败隔离、按夹提交与变更版本、账号任务取消；性能先测量 | I-07、I-08、I-10、I-13、I-14、I-15、I-16/I-17 的恢复；A-03、A-04、A-06 |
| 4. 协议完整性及验证 | 完善 MIME/回复头/plain、特殊文件夹与 All 展示范围、Sent APPEND/服务器保存、SMTP 错误分类和 HTML 隔离；验证 Graph/IMAP 同位置幂等、不同位置独立与操作安全 | I-01、I-11、I-12、I-18、I-19；A-01 至 A-06 回归 |

每项按“领域规则 → Repository 模块函数契约 → application 编排与 infrastructure → 必要的 IPC/前端 Port 与 Adapter”实施。V1 可重建开发库和重新登录，不为旧字段或旧 IPC 保留兼容链。

阶段允许必要的前置依赖，同一问题可先止损、再完善恢复和验证，不重复计为新问题。A-02 随核心模型调整，不留到最后搬类型；统一定位直接放在 Message，移动沿用位置移除/新增，不增加身份保留映射、位置集合表或跨文件夹合并任务，尽量一次改完相关表和调用方。通用 OAuth2/XOAUTH2 的完整能力不进入 V1 必须完成顺序；但拒绝不支持的配置是 V1 入口正确性要求。Graph OAuth 保留。全量 UID 差集、历史主动续跑、FLAGS 批量核对及 INBOX IDLE 策略继续有效，不恢复为未实现项。

## 5. 验证缺口

现有 `domain/services/imap_sync_policy.rs` 的 5 项纯规则测试覆盖稀疏 UID、跨批次补齐、失败保留、空邮箱/删除和 UID 上界；`tests/imap_history_sync.rs` 验证已提交进度与启用文件夹驱动续跑。另有协议配置/游标与 Repository 事务测试。此前复核记录共 17 项通过，本轮仅检查相关测试定义、未重跑，不将历史结果用于证明本次新增模型规则已满足。

仍需补充：

1. 模拟协议交互：安全删除、缺正文/解析失败、整页坏信/批次网络错误、IDLE 超时与取消、SMTP 临时/永久拒绝和不确定结果。纯集合测试不替代真实解析与网络交互测试。V1 验证通用 OAuth2 配置/IPC/后台路径被明确拒绝且无副作用、Graph OAuth 不受影响，不要求通用刷新/轮换/撤销测试。
2. 统一模型与 SQLite：Message 最多一个有效位置、三列唯一键和并发重复同步幂等；不同账号/不同位置的 Message 永远独立，头/正文全同不合并，A→B 的 Sent/INBOX 独立状态与任务。所有移动（含 Postlet 发起）均源位置移除、目标位置新增，目标不继承源 MessageId；复制保留源并在目标新建，操作返回与同步重复发现目标均按位置幂等。覆盖原生移动能力不足、UIDVALIDITY 不匹配、移动成功后 SQL 失败、最终应答丢失及重启核对，不盲目重做或扩大远端操作范围；异常头/回复关系只影响既有会话判断、generation/定位失效、旧任务不追到目标、已读反向操作和旧 FLAGS 晚到、lease 恢复；文件夹重命名/删除重建/角色变化不继承旧任务。改变/不提供 Graph 专有身份字段或停用 Delta 不改变业务决策。另测退出/清理竞态、外部凭据补偿。
3. MIME/特殊文件夹：嵌套结构、disposition、CID、地址组、日期、完整 plain；SPECIAL-USE/无扩展、INBOX 语义、Draft FLAG 与 Drafts 属性、非英文名称/角色手动映射。Graph/IMAP 同角色规则一致，INBOX/Archive/All 各自独立入库，All 默认排除、显式范围展示；同内容不同位置的已读/删除独立，不要求底层去重。
4. 附件故障注入：临时写/原子移动/SQL 事务各阶段失败和崩溃重启，孤儿清理与无效引用、发送重试持久内容、多消息共享引用、回收与导入/运行任务竞态、账号退出后的保留策略；接收端字节一致且不静默缺附件发送。
5. HTML 安全：检查实际安装的 Sanitizer 版本与策略，构造脚本/事件/危险 URL/畸形 HTML/CSS 样例；捕获打开和清洗期间的网络请求，验证默认屏蔽、授权范围、CID/本地资源越权、外链与 HTML 附件隔离。分别在 Tauri 开发/生产检查 CSP、窗口/子上下文 capabilities 与 IPC 拒绝；目前未运行，不声称已证实 XSS、IPC 利用或 HTML 附件越权。
6. 受控端到端与性能：大邮箱基准、服务器收取范围、不同 IDLE/UIDPLUS/MOVE、两类 Sent 行为。未来采用发现高水位/UIDNEXT 时，对照集合基线验证发现后崩溃、下载失败、稀疏 UID、FLAGS/删除和周期补漏；不把优化缺失本身列为发布阻塞，也不把服务器不可见邮件误判成分页失败。
7. 发送/归档恢复：首次网络调用前 Message-ID/MIME 已持久化；确定拒绝才自动重发，接受后本地回滚/最终应答丢失不盲目重发；APPEND 失败只重试归档，未知结果不盲目重做。自动保存 Sent 按位置独立入库，不按头/正文匹配；确定操作结果绑定无位置发送记录或引用已存在 Sent 实体，不合并实体，普通会话无位置发送记录展示符合 I-11。
8. ChangeSet/通知一致性：commit 前失败、commit 后分发前崩溃、漏事件/重订阅/查询乱序、两批成功后第三批失败；版本与数据同快照，已提交批次均可重查恢复，系统通知尽力而为且重启不补放历史，不以通知失败回滚收信或重做协议操作。

只在对应行为和失败路径验证后移除问题；“能收新邮件”“网页已读后来一致”或“增加日志”均不能等同于所有收发与恢复问题已解决。

## 6. V1 暂不支持及后续规划

### I-09 / P2（V1 暂缓）：通用 OAuth2/XOAUTH2 分支不可视为已支持

**状态**：暂缓完整能力，未修复；从 V1 必须实现功能列表移出，不计为已解决。显式拒绝该配置的 V1 必要工作归 I-17/A-05。

**技术事实与依据**：`imap/client.rs::Xoauth2::process` 自己 Base64 编码，锁定 imap 2.4.1 的 authenticate 再编码，存在双重编码。`MailSecret` 虽有 refresh_token/expires_at_ms，通用 IMAP/SMTP 仅读取 access_token。当前 AddAccountDialog 通用账号固定 password，但 `UpsertImapSmtpAccountCommand`、`mail_account_use_case` 和协议执行分支仍允许 OAuth2，后端尚未显式禁止。这些是静态代码结论，没有成功认证/攻击复现。

**V1 边界与处理**：新通用邮箱只支持密码、应用密码/邮箱授权码，UI 不宣称支持 OAuth2，IPC/用例和后台对不支持的配置明确报错，不进入错误 XOAUTH2 流程、不静默换认证方式。不能只删除共享枚举的 OAuth2 而破坏 Graph；Graph OAuth 登录及已有令牌管理保留。

**本期不要求**：通用 access_token 刷新、refresh_token 轮换/撤销/并发刷新、多服务商授权端点或 scopes 适配，均不作为 V1 发布阻塞或立即实施任务。未来确有需求时再评估启用范围，并在启用前处理已记录的编码缺陷及重新定义验收，不保留误导性的半支持对外入口。

**当前验收**：通用 OAuth2 请求明确不支持且无写库/网络副作用；密码/授权码路径仍可用；Graph OAuth 不受影响。完整通用 OAuth2 能力暂不安排实现或宣称测试通过。
