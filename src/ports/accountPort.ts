import type { Account } from '../domain/entities';
import type { UpdateAccountDisplaySettingsCommand } from '../domain/dto';
import type { AccountId } from '../domain/types';

/** 账户入站端口 */
export interface AccountPort {
  /** 列出已登录账户 */
  listAccounts(): Promise<Account[]>;
  getAccount(accountId: AccountId): Promise<Account>;
  signIn(): Promise<Account>;
  signOut(accountId: AccountId): Promise<void>;
  updateDisplaySettings(
    cmd: UpdateAccountDisplaySettingsCommand,
  ): Promise<Account>;
  /**
   * 开发验收：清空并装载演示工作区。
   * Memory 实现完整数据；Tauri 可返回 not_implemented。
   */
  loadDemoWorkspace(): Promise<Account>;
}
