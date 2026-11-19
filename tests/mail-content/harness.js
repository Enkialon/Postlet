import { createApp, h, reactive } from 'vue';
import MailBodyView from '/src/shared/mail-content/MailBodyView.vue';
import { portsKey } from '/src/app/provide.ts';

const state = reactive({ content: '<p>initial</p>', contentType: 'html', folderRole: null, key: 'account:message-1' });
window.setMail = value => Object.assign(state, value);
window.opened = [];
// Deliberately expose a privileged parent entry point; mail must not execute it.
window.__TAURI_INTERNALS__ = { invoke: () => { window.compromised = true; } };
createApp({ render: () => h(MailBodyView, state) })
  .provide(portsKey, { platform: { openExternalUrl: async url => window.opened.push(url) } })
  .directive('no-i18n', {})
  .mount('#app');
