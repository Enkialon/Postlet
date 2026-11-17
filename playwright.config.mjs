import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './tests/mail-content',
  projects: [{ name: 'dev-csp' }, { name: 'production-csp' }],
  use: {
    baseURL: 'http://127.0.0.1:1430',
    channel: process.platform === 'win32' ? 'msedge' : 'chromium',
  },
  webServer: {
    command: 'node node_modules/vite/bin/vite.js --host 127.0.0.1 --port 1430',
    url: 'http://127.0.0.1:1430',
    reuseExistingServer: false,
  },
});
