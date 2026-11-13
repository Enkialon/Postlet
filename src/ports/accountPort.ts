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
}
