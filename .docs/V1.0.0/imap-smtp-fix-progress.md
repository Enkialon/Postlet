# IMAP / SMTP 修复记录

依据 `imap-smtp-architecture-review.md` 的 2026-11-17 工作区版本实施。原问题文档及规则已有未提交修改，本记录单独维护修复证据。仅完成对应验收后标为解决；止损不代表完整问题解决。

## I-01：已解决

- 删除前要求 UIDPLUS；能力缺失时不 SELECT、不 STORE、不产生删除副作用。
- 仅对目标 UID STORE / UID EXPUNGE；任何错误均不回退整夹 EXPUNGE。
- STORE 应答不确定及 UID EXPUNGE 失败明确报告目标可能/已经标记 Deleted，由现有 mutation last_error 保留。
- 验证：`cargo test --manifest-path src-tauri/Cargo.toml --lib infrastructure::imap::client::tests -- --nocapture`，2 项通过。模拟不支持 UIDPLUS、UID EXPUNGE 成功、NO、BAD、断线，核对命令范围。未连接真实邮箱。

## I-16：已修复，Windows 已验证

- 通用凭据和 Graph token 共用 AES-256-GCM 加密存储；随机 nonce、账号/用途认证绑定，系统钥匙串保管密钥，磁盘不保存明文。密钥缺失不静默替换已有密文的密钥；首次创建用跨进程文件锁串行。
- 临时密文写入并 sync 后原子替换；不读取旧明文开发凭据，无兼容分支。旧开发 JSON 需自行清理并重新登录，未自动删除已有文件。
- 秘密实体、认证参数、IPC 凭据 DTO、token 响应移除 Debug；退出删除通用凭据失败返回错误，可以重试。
- 移除未调用的扫描 JSON 推断登录账号入口；登录态仍以 SQLite 为准。
- 验证：13 项 Rust 单元测试通过；另外 `cargo test --manifest-path src-tauri/Cargo.toml --lib infrastructure::secret_store::tests --offline -- --include-ignored` 3 项通过，包含真实 Windows 凭据管理器隔离密钥的创建/读写/删除，以及大 token、替换、密文篡改、跨账号拒绝和失败保留测试。未验证 macOS/Linux 钥匙串及断电持久性。

## I-17：已解决

- 参数、端点、端口、TLS 与密码先校验，通用 OAuth2 在任何文件/数据库操作前拒绝。IMAP/SMTP Adapter 也在连接前拒绝 OAuth2；共享 Graph OAuth 枚举和 Graph token 路径保留。
- 每次配置写独立随机凭据引用；新秘密可读后，事务一起切换账号与传输配置。SQL/COMMIT 失败确认引用后清理新秘密，旧配置和秘密保持有效；成功切换后回收旧引用，回收失败明确报告已保存与待清理。
- 启动时在同一跨进程配置锁下核对并回收无引用的通用凭据，覆盖暂存后崩溃和旧凭据回收前崩溃。退出与配置切换使用同一把锁。
- 按 V1 直接修订初始数据库，不提供旧配置/明文凭据兼容；已有开发库需要重建。
- 验证：`cargo test --manifest-path src-tauri/Cargo.toml --offline --lib --tests` 共 23 项通过（2 项系统钥匙串测试默认跳过）；随后原生配置测试 2 项通过，覆盖存储失败、SQL 失败、延迟外键导致 COMMIT 失败、旧引用保留、孤儿恢复、更新切换、退出删除失败与重试。最终 `--lib -- --include-ignored` 16 项通过，含 IMAP/SMTP 连接前 OAuth2 拒绝及 Graph 命名空间凭据回归。未连接真实邮箱或执行交互式 Graph OAuth 登录。

## I-19：防护已实现，Edge 验证通过；Tauri 实机安全验收仍待完成

- 替换 vue-letter 为精确锁定的 DOMPurify 3.4.18。原始 HTML 只在 template 惰性文档中解析并原地清洗，不进入主应用 DOM。显式标签/属性白名单，移除脚本、嵌套页面、表单、SVG/MathML、样式表及能加载资源的 CSS；保留基础排版样式。
- 清洗结果进入 sandbox iframe；仅 allow-same-origin 供父页面测量/链接处理，不授予 allow-scripts。正文 CSP 禁止脚本、表单和其他默认资源；默认删除图片 src 并禁止 img-src，允许单封授权远程图片，拒绝 cid/file/asset/data 和 srcset。切换邮件（含相同正文）重建组件，清除授权；可再次屏蔽。
- 链接不保留真实浏览器导航地址，由父页面对用户点击再次验证后调用 PlatformPort；仅 http/https/mailto。没有新增 HTML 附件同源预览或开放任意 CID。
- 主窗口配置开发/生产 CSP，未增加任何远程 capability。旧邮件渲染依赖与全局样式已移除。
- 验证：Edge + Playwright 8 项通过，分别使用实际配置中的开发/生产 CSP，捕获解析/渲染/授权时图片与 CSS/字体/iframe 等请求；覆盖危险 URL、畸形 HTML、CID/本地资源拒绝、授权撤销和跨邮件重置、链接 Port、新窗口及绕过清洗直接注入脚本后的独立 sandbox/CSP 防护。`pnpm build` 和 `cargo check --manifest-path src-tauri/Cargo.toml --offline` 通过。
- 尚未在 Tauri 安装包内验证 WebView2/其他平台的真实 IPC 拒绝与 capabilities 执行，浏览器父页面模拟的高权限入口不能代替该验收，因此不将整项标为完全解决。HTML 附件预览当前不存在，未来实现必须复用同等隔离。

## I-07：已止损，完整附件生命周期尚未实现

- Compose 入口在数据库/草稿修改前拒绝带附件发送，现有 UI 错误路径保留用户草稿；outbound 对已有附件标记再次拒绝，不能静默去掉附件后发往 Graph/SMTP。
- 后续仍需统一受管理内容、导入 Port、持久引用、重启恢复、回收及两种发送协议接线；不能将明确拒绝发送等同于附件功能完成。
- 验证：Rust 编译及现有测试回归；未执行真实附件收发，完整生命周期仍未实现。

## A-01：身份按位置拆分已接通，核心模型仍在继续完善

- Message/RemoteMessage/前端模型以 RemoteKey 替换 GraphImmutableId/graph_id，删除 graph_change_key、created_at_graph；Graph 特有账户/联系人适配结果留在 infrastructure，统一邮件事实归无 IO 的 domain。
- 仓储查找和数据库唯一键改为 `(account_id, folder_id, remote_key)`；相同内容/RFC Message-ID 不合并位置。移除 Graph Remove 后跨文件夹追踪、替换原 Message 位置的 locate 表与任务；源位置移出保留本地内容，目标按其位置独立入库。
- 同代来源位置被权威重新发现时允许恢复原实体，不再因为旧 DeletedRemote 状态永久拒绝恢复。
- 验证：Rust 26 项通过（2 项系统钥匙串用例默认跳过），新增同位置幂等、不同文件夹/账号独立、源移出不跟随到目标、原内容保留、同位置数据库唯一约束测试；前端构建通过。Graph 映射回归同时检查专有元数据变化不改变统一结果。
- 仍需本地 FolderId/远端绑定分离、显式定位有效性与修订、generation 及固定 mutation 目标；当前不是 A-01/A-06/I-06 全部完成，后续继续同方向修改，不保留旧命名兼容层。

## I-06：UIDVALIDITY 隔离与固定目标已接通，定位修订保护仍在完善

- IMAP RemoteKey 编码 UIDVALIDITY + UID，只有 IMAP infrastructure 解码；application 不再解析 IMAP 标识。新代相同 UID 产生不同 RemoteKey/MessageId。
- 已读和安全永久删除在实际执行的同一连接 SELECT 后严格核对预期 UIDVALIDITY，缺失/不匹配均停止，绝不 STORE/EXPUNGE；没有有效 UIDVALIDITY 的同步也拒绝继续。
- Folder 单独保存不透明 generation，下载集合只信任当前代。代次变化时同事务停用旧代消息展示及旧任务、保留正文和错误，再入库新代邮件及进度。
- mutation 纯数据移入 domain，入队保存 target_folder_id/target_remote_key，执行不重新读取消息来改目标。终止任务显式 is_active=false，停止自动派发及本地意图覆盖，错误保留；非重试错误也终止。无远端定位不生成远端任务。
- 验证：本轮全套 Rust 含原生钥匙串 31 项通过；补充显式终止标记后再次通过 20 项单元测试（原生钥匙串 1 项默认跳过）及前端构建。覆盖 UIDVALIDITY 缺失/变化时无写命令、非法 key、新旧 UID 10 实体隔离、旧删除/已读意图不继承、旧目标停止且正文保留。
- 仍需与本地 Folder 身份、定位状态/修订和任务 lease 一起完成重命名/退役/并发晚到保护；不能将以上测试等同于 I-06/I-10 全部验收完成。

## A-02：已解决

- 技术错误统一归 infrastructure::error::InfraError，application 映射为 AppError；删除旧应用层 PortError，无兼容别名。
- 邮件事实与 mutation 为纯 domain 类型；Graph 账户/联系人适配结果和仓储任务 lease 返回值归 infrastructure。
- 仓储接收实际筛选/分页参数，返回实体或必要查询投影；会话展示名与 MessagePreview/SessionListItem 由 application 组装，IPC 查询 DTO 保留在 application。
- 只有确实跨层复用的 CursorPage 放在无 IO 的小型 query_page 模块；没有新增接口层、Repository trait 或事务管理对象。
- 验证：infrastructure 全目录无 application 导入，domain 无反向依赖。`cargo test --manifest-path src-tauri/Cargo.toml --offline --lib --tests` 29 项通过、2 项原生钥匙串测试默认跳过；覆盖列表展示、账号隔离和跨仓储事务提交/回滚。

## A-06：显式 FLAGS 与真实观测已接通，发送/归档状态拆分仍待实现

- 使用统一 RemoteMessageChange::FlagsChanged，移除空标题/空正文占位和启发式分支。合法空标题快照正常更新标题；FLAGS 更新保留正文、收件人、时间及 Session。
- RemoteFlags 的每个观测均可未知；Message 单独保存远端 Seen/Draft/Deleted 观测，最新有效本地已读意图只覆盖展示。Deleted FLAG 不等同于位置消失或用户删除；终止意图不继续遮盖真实观测。
- IMAP 复用已锁定的 imap-proto 解析器核对完整 FLAGS 属性：未返回 FLAGS 不更新，显式 FLAGS () 才清除；新下载和既有消息一起按 200 UID 批量核对。正文 FETCH 不再用缺省空 flags 伪造已观测 false。
- Graph 稀疏 Delta 先在原文件夹范围内补完整快照，404 仅移出原位置，不跨夹追踪或用空值覆盖。Graph 缺失标志保持未知。只有明确非 Draft 的新增消息才具备通知资格。
- 验证：`cargo test --manifest-path src-tauri/Cargo.toml --offline --lib --tests` 31 项通过、2 项原生钥匙串测试默认跳过；`pnpm build` 通过。覆盖缺失/空/完整 FLAGS、危险误用序号、非法响应、观测与本地意图并存、字段保留、空标题及 Graph 稀疏快照分类。Graph HTTP 补取路径尚未连接真实邮箱验证。
- 发送任务、归档任务及定位状态/修订仍需后续拆分，未将整项 A-06 标为解决。

## A-01 / I-06：本地 Folder 身份与定位有效性已接通（追加）

- FolderId 由本地生成，远端绑定、协议、generation、绑定修订及退役状态独立保存。完整目录发现按有效绑定复用本地 ID；退役后的同名路径新建 ID，不继承勾选、游标或旧任务。父子关系使用同账号本地 ID。游标字段改为通用 sync_cursor/page_cursor。
- Message 定位显式区分 present/missing/invalidated 并保存单调修订。三列位置唯一键覆盖失效旧记录；字段组 CHECK 与同账号 Folder 外键约束归属。数据库禁止替换既有位置、倒退修订及复活 invalidated 定位；无位置发送记录首次绑定确定位置仍允许。
- 位置消失/代次失效保留正文、附件标记、Session 和错误，普通列表/预览/未读排除无效位置。missing 同代权威重现可恢复原实体并升修订；invalidated 不恢复。
- mutation 同事务冻结位置与绑定修订；派发前校验登录态、协议、有效位置、目标及修订，失效时保留错误记录并停止，不读取当前路径来改写目标。展示也只覆盖仍有效的目标意图。
- IMAP 请求用远端绑定 SELECT、用本地 ID 输出事实；Graph 读取/写入采用已核对的原文件夹范围，不跨位置追踪。异步正文补取仅条件更新正文，不能用旧整行覆盖最新已读/定位状态。旧绑定同步结果在提交前拒绝。
- 变更收信主机、端口、TLS、用户名或协议时，同一配置事务退役原文件夹、失效位置和停止任务；仅替换密码保留原身份。SQL 失败时这些变更和新配置一起回滚。
- Graph 目录发现使用完整 BFS；缺失绑定/子目录数量或中途失败不提交退役，根及子目录都显式包括隐藏目录，避免不同枚举范围误判删除。依据 [List mailFolders](https://learn.microsoft.com/en-us/graph/api/user-list-mailfolders) 与 [List childFolders](https://learn.microsoft.com/en-us/graph/api/mailfolder-list-childfolders)；邮件内容 Delta 保留，未增加 folder Delta 状态双轨。
- 验证：`cargo test --manifest-path src-tauri/Cargo.toml --offline --lib --tests -- --include-ignored` 36 项通过，含真实 Windows 钥匙串；前端构建通过。新增同名重建/父子本地 ID、退役不可复活、绑定/位置修订拒绝旧任务、位置不可替换、首次绑定、同账号 Folder 外键、端点变更原子回滚/密码替换保留身份，以及不把未知子目录数量视为 0 的测试。
- 尚需 I-10/A-03/I-14 处理网络请求串行、认领恢复、旧 FLAGS 晚到、前端失败展示、账号任务取消及提交版本；协议真实收发和 Tauri 安装包安全验收仍不以以上模型测试替代。
