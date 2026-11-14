import { computed, inject, ref } from 'vue';
import type { Account } from '../../domain/entities';
import type { UpsertImapSmtpAccountCommand } from '../../domain/dto';
import type { AppError } from '../../domain/errors';
import type { AccountFilter } from '../../app/navigation';
import { portsKey, navigationKey } from '../../app/provide';
import { asAppError } from '../../shared/errors/asAppError';

const accounts = ref<Account[]>([]);
const current = ref<Account | null>(null);
const loading = ref(false);
const signingIn = ref(false);
const error = ref<AppError | null>(null);

export function useAccounts() {
  const ports = inject(portsKey)!;
  const nav = inject(navigationKey)!;

  const hasAccounts = computed(() => accounts.value.length > 0);

  const filterAccount = computed(() => {
    const f = nav.accountFilter.value;
    if (f === 'all') return null;
    return accounts.value.find((a) => a.id === f) ?? null;
  });

  function syncCurrentFromNav(): void {
    const id = nav.accountId.value;
    current.value = id
      ? (accounts.value.find((a) => a.id === id) ?? null)
      : null;
  }

  async function bootstrapAccounts(): Promise<void> {
    loading.value = true;
    error.value = null;
    try {
      const list = await ports.account.listAccounts();
      accounts.value = list;
      if (list.length === 0) {
        current.value = null;
        nav.setAccount(null);
        nav.setAccountFilter('all');
        nav.goWelcome();
        return;
      }
      const savedFilter = nav.readAccountFilter();
      const filter: AccountFilter =
        savedFilter === 'all' || list.some((a) => a.id === savedFilter)
          ? savedFilter
          : 'all';
      nav.setAccountFilter(filter);
      const last = nav.readLastAccountId();
      const contextId =
        (last && list.find((a) => a.id === last)?.id) ||
        (filter !== 'all' ? filter : list[0]!.id);
      nav.setAccount(contextId);
      syncCurrentFromNav();
      if (nav.page.value === 'welcome') nav.goPage('conversations');
      nav.bumpSession();
    } catch (e) {
      error.value = asAppError(e);
    } finally {
      loading.value = false;
    }
  }

  /** 左上角：全部或某一账号（只改列表过滤；读写信仍跟会话归属） */
  async function setAccountFilter(filter: AccountFilter): Promise<void> {
    nav.bumpSession();
    nav.clearReading();
    nav.setAccountFilter(filter);
    if (filter !== 'all') {
      try {
        const account = await ports.account.getAccount(filter);
        current.value = account;
        nav.setAccount(account.id);
      } catch (e) {
        error.value = asAppError(e);
      }
    } else {
      syncCurrentFromNav();
    }
    if (nav.page.value === 'welcome') nav.goPage('conversations');
  }

  /** @deprecated 用 setAccountFilter；保留给仍传具体 id 的调用 */
  async function selectAccount(
    accountId: string,
    options?: { triggerSync?: boolean },
  ): Promise<void> {
    await setAccountFilter(accountId);
    if (options?.triggerSync !== false) {
      void ports.sync.triggerMailSync({
        account_id: accountId,
        folder_ids: null,
      });
      void ports.sync.triggerContactSync({ account_id: accountId });
    }
  }

  /** 选中会话/联系人后锁定读写上下文账号 */
  function setContextAccount(accountId: string): void {
    nav.setAccount(accountId);
    syncCurrentFromNav();
  }

  async function afterAccountReady(account: Account): Promise<void> {
    accounts.value = await ports.account.listAccounts();
    nav.setAccount(account.id);
    // 多账号时保持「全部」过滤，便于立刻看到所有邮箱
    if (accounts.value.length > 1) {
      nav.setAccountFilter('all');
    } else {
      nav.setAccountFilter(account.id);
    }
    syncCurrentFromNav();
    nav.bumpSession();
    nav.clearReading();
    nav.goPage('conversations');
    void ports.sync.triggerMailSync({
      account_id: account.id,
      folder_ids: null,
    });
    void ports.sync.triggerContactSync({ account_id: account.id });
  }

  async function signIn(): Promise<void> {
    if (signingIn.value) return;
    signingIn.value = true;
    error.value = null;
    try {
      const account = await ports.account.signIn();
      await afterAccountReady(account);
    } catch (e) {
      error.value = asAppError(e);
    } finally {
      signingIn.value = false;
    }
  }

  async function addImapSmtpAccount(
    cmd: UpsertImapSmtpAccountCommand,
  ): Promise<void> {
    if (signingIn.value) return;
    signingIn.value = true;
    error.value = null;
    try {
      const account = await ports.account.upsertImapSmtpAccount(cmd);
      await afterAccountReady(account);
    } catch (e) {
      error.value = asAppError(e);
    } finally {
      signingIn.value = false;
    }
  }

  async function loadDemoWorkspace(): Promise<void> {
    if (signingIn.value) return;
    signingIn.value = true;
    error.value = null;
    try {
      const account = await ports.account.loadDemoWorkspace();
      accounts.value = await ports.account.listAccounts();
      nav.setAccountFilter(account.id);
      nav.setAccount(account.id);
      syncCurrentFromNav();
      nav.bumpSession();
      nav.clearReading();
      nav.goPage('conversations');
      void ports.sync.triggerMailSync({
        account_id: account.id,
        folder_ids: null,
      });
      void ports.sync.triggerContactSync({ account_id: account.id });
    } catch (e) {
      error.value = asAppError(e);
    } finally {
      signingIn.value = false;
    }
  }

  async function signOut(): Promise<void> {
    const id =
      nav.accountFilter.value !== 'all'
        ? nav.accountFilter.value
        : nav.accountId.value;
    if (!id) return;
    error.value = null;
    try {
      await ports.account.signOut(id);
      accounts.value = await ports.account.listAccounts();
      if (accounts.value.length === 0) {
        current.value = null;
        nav.bumpSession();
        nav.setAccount(null);
        nav.setAccountFilter('all');
        nav.clearReading();
        nav.goWelcome();
        return;
      }
      nav.setAccountFilter('all');
      nav.setAccount(accounts.value[0]!.id);
      syncCurrentFromNav();
      nav.bumpSession();
      nav.clearReading();
    } catch (e) {
      error.value = asAppError(e);
    }
  }

  async function refreshCurrent(): Promise<void> {
    if (!current.value) return;
    current.value = await ports.account.getAccount(current.value.id);
  }

  function replaceCurrent(account: Account): void {
    current.value = account;
    accounts.value = accounts.value.map((a) =>
      a.id === account.id ? account : a,
    );
  }

  function accountLabel(accountId: string): string {
    const a = accounts.value.find((x) => x.id === accountId);
    return a?.display_name || a?.email || accountId;
  }

  return {
    accounts,
    current,
    filterAccount,
    loading,
    signingIn,
    error,
    hasAccounts,
    bootstrapAccounts,
    setAccountFilter,
    selectAccount,
    setContextAccount,
    accountLabel,
    signIn,
    addImapSmtpAccount,
    loadDemoWorkspace,
    signOut,
    refreshCurrent,
    replaceCurrent,
  };
}
