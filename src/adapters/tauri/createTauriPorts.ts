import type { AppPorts } from '../../ports';
import { normalizeAppError } from './normalizeError';

/**
 * Tauri Adapter 占位：IPC command 名称尚未定稿（见 frontend.md §10）。
 * 组合根在生产环境可切换至此；当前开发默认使用内存实现。
 */
export function createTauriPorts(): AppPorts {
  const notReady = async (): Promise<never> => {
    throw normalizeAppError({
      code: 'internal',
      message: 'Tauri IPC 尚未接入，请使用内存 Adapter 开发',
      retryable: false,
    });
  };

  return {
    account: {
      listAccounts: notReady,
      getAccount: notReady,
      signIn: notReady,
      signOut: notReady,
      updateDisplaySettings: notReady,
      loadDemoWorkspace: notReady,
    },
    sync: {
      triggerMailSync: notReady,
      triggerContactSync: notReady,
      getMailSyncProgress: notReady,
      getContactSyncProgress: notReady,
    },
    contact: {
      listContacts: notReady,
      getContact: notReady,
      getContactByAddress: notReady,
    },
    conversation: {
      listConversations: notReady,
      getConversation: notReady,
      listMessages: notReady,
      ensureContactConversation: notReady,
      ensureGroupConversation: notReady,
    },
    message: {
      getMessage: notReady,
      ensureMessageBody: notReady,
      markRead: notReady,
    },
    group: {
      listGroups: notReady,
      getGroup: notReady,
      listParticipants: notReady,
      createManualGroup: notReady,
      renameGroup: notReady,
      mergeGroups: notReady,
      moveThreadToGroup: notReady,
      updateSendMembers: notReady,
    },
    platform: {
      openExternalUrl: async (url: string) => {
        const { openUrl } = await import('@tauri-apps/plugin-opener');
        if (!/^https?:\/\//i.test(url.trim())) {
          throw normalizeAppError({
            code: 'validation',
            message: '仅允许 http / https 外链',
            retryable: false,
          });
        }
        await openUrl(url.trim());
      },
    },
  };
}
