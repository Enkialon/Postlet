import type { Account } from '../domain/entities';
import type {
  UpdateAccountAliasesCommand,
  UpdateAccountDisplaySettingsCommand,
  UpsertImapSmtpAccountCommand,
} from '../domain/dto';
import type { AccountId } from '../domain/types';

/** 账户入站端口 */
export interface AccountPort {
  /** 列出已登录账户 */
  listAccounts(): Promise<Account[]>;
  getAccount(accountId: AccountId): Promise<Account>;
  /** Outlook / Graph OAuth 登录 */
  signIn(): Promise<Account>;
  /** IMAP + SMTP 账号配置与凭证落盘 */
  upsertImapSmtpAccount(cmd: UpsertImapSmtpAccountCommand): Promise<Account>;
  signOut(accountId: AccountId): Promise<void>;
  /** 清理本地聊天/同步数据；保留账号与登录态 */
  clearLocalData(accountId: AccountId): Promise<void>;
  updateDisplaySettings(
    cmd: UpdateAccountDisplaySettingsCommand,
  ): Promise<Account>;
  /** 手动维护别名（视为本账号；不做 Send-As） */
  updateAliases(cmd: UpdateAccountAliasesCommand): Promise<Account>;
  /** 账号当前持久变更版本；用于校验前端快照新鲜度 */
  getChangeVersion(accountId: AccountId): Promise<number>;
}
