/** Unix 毫秒时间戳 */
export type TimestampMs = number;
/** 规范化邮箱 */
export type EmailAddress = string;
export type AccountId = string;
export type ContactId = string;
export type MessageId = string;
export type GroupId = string;
export type SessionId = string;
export type RemoteKey = string;
export type FolderId = string;

/** 账户 / 文件夹同步状态 */
export type SyncStatus = 'idle' | 'running' | 'error';
/** 单封邮件同步状态 */
export type MessageSyncStatus = 'synced' | 'pending_local' | 'deleted_remote';
export type BodyContentType = 'text' | 'html';
export type Importance = 'low' | 'normal' | 'high';
export type RecipientRole = 'to' | 'cc' | 'bcc' | 'reply_to';
/** 群组诞生方式 */
export type GroupSource = 'auto' | 'manual';
/** 联系人来源：真联系人 vs 邮件/群观测档 */
export type ContactSource = 'graph' | 'manual' | 'observed';
/** 会话种类：一张 sessions 表内区分 */
export type SessionKind = 'private' | 'group';
export type AggregateJobStatus =
  | 'pending'
  | 'processing'
  | 'completed'
  | 'retry'
  | 'failed';
export type AggregationOutcome = 'completed' | 'partial' | 'deferred' | 'skipped';
export type PageDirection = 'older' | 'newer';
/** 收信协议（按账号配置，非服务商品牌） */
export type ReceiveProtocol = 'graph' | 'imap';
/** 发信协议 */
export type SendProtocol = 'graph' | 'smtp';
/** 邮箱认证方式 */
export type MailAuthKind = 'password' | 'oauth2';
/** TLS 模式 */
export type MailTlsMode = 'implicit' | 'starttls' | 'plain';
