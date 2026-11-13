import { computed, inject, ref } from 'vue';
import type { Account } from '../../domain/entities';
import type { AppError } from '../../domain/errors';
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

  async function bootstrapAccounts(): Promise<void> {
    loading.value = true;
    error.value = null;
    try {
      const list = await ports.account.listAccounts();
      accounts.value = list;
      if (list.length === 0) {
        current.value = null;
        nav.setAccount(null);
        nav.goWelcome();
        return;
      }
      const last = nav.readLastAccountId();
      const picked =
        (last && list.find((a) => a.id === last)) || list[0] || null;
      await selectAccount(picked!.id, { triggerSync: false });
    } catch (e) {
      error.value = asAppError(e);
    } finally {
      loading.value = false;
    }
  }

  async function selectAccount(
    accountId: string,
    options?: { triggerSync?: boolean },
  ): Promise<void> {
    nav.bumpSession();
    nav.clearReading();
    const account = await ports.account.getAccount(accountId);
    current.value = account;
    nav.setAccount(account.id);
    if (nav.page.value === 'welcome') nav.goPage('conversations');
    if (options?.triggerSync !== false) {
      void ports.sync.triggerMailSync({
        account_id: account.id,
        folder_ids: null,
      });
      void ports.sync.triggerContactSync({ account_id: account.id });
    }
  }

  async function signIn(): Promise<void> {
    if (signingIn.value) return;
    signingIn.value = true;
    error.value = null;
    try {
      const account = await ports.account.signIn();
      accounts.value = await ports.account.listAccounts();
      await selectAccount(account.id, { triggerSync: true });
      nav.goPage('conversations');
    } catch (e) {
      error.value = asAppError(e);
    } finally {
      signingIn.value = false;
    }
  }

  async function signOut(): Promise<void> {
    const id = current.value?.id;
    if (!id) return;
    error.value = null;
    try {
      await ports.account.signOut(id);
      accounts.value = await ports.account.listAccounts();
      if (accounts.value.length === 0) {
        current.value = null;
        nav.bumpSession();
        nav.setAccount(null);
        nav.clearReading();
        nav.goWelcome();
        return;
      }
      await selectAccount(accounts.value[0]!.id, { triggerSync: false });
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

  return {
    accounts,
    current,
    loading,
    signingIn,
    error,
    hasAccounts,
    bootstrapAccounts,
    selectAccount,
    signIn,
    signOut,
    refreshCurrent,
    replaceCurrent,
  };
}
