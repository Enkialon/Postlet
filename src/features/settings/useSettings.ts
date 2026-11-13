import { inject, ref } from 'vue';
import { portsKey } from '../../app/provide';
import { useAccounts } from '../accounts/useAccounts';
import { useConversations } from '../conversations/useConversations';
import { useMessages } from '../conversations/useMessages';
import { usePreferences } from '../../app/preferences';
import type { AppLocale } from '../../i18n';
import { asAppError } from '../../shared/errors/asAppError';

export function useSettings() {
  const ports = inject(portsKey)!;
  const accounts = useAccounts();
  const conversations = useConversations();
  const messages = useMessages();
  const prefs = usePreferences();
  const saving = ref(false);
  const error = ref<string | null>(null);

  async function setShowGroupInContact(value: boolean): Promise<void> {
    const account = accounts.current.value;
    if (!account || saving.value) return;
    const previous = account.show_group_messages_in_contact_conversation;
    accounts.replaceCurrent({
      ...account,
      show_group_messages_in_contact_conversation: value,
    });
    saving.value = true;
    error.value = null;
    try {
      const updated = await ports.account.updateDisplaySettings({
        account_id: account.id,
        show_group_messages_in_contact_conversation: value,
        include_in_all: account.include_in_all,
      });
      accounts.replaceCurrent(updated);
      conversations.invalidate();
      await messages.loadInitial();
    } catch (e) {
      accounts.replaceCurrent({
        ...account,
        show_group_messages_in_contact_conversation: previous,
      });
      error.value = asAppError(e).message;
    } finally {
      saving.value = false;
    }
  }

  function setLocale(locale: AppLocale): void {
    prefs.setLocale(locale);
  }

  return {
    accounts,
    prefs,
    saving,
    error,
    setShowGroupInContact,
    setLocale,
  };
}
