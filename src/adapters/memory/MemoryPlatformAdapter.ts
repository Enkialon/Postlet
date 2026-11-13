import type { PlatformPort } from '../../ports/platformPort';

/** 开发环境：在新窗口打开外链 */
export class MemoryPlatformAdapter implements PlatformPort {
  async openExternalUrl(url: string): Promise<void> {
    const trimmed = url.trim();
    if (!/^https?:\/\//i.test(trimmed)) {
      throw {
        code: 'validation',
        message: '仅允许 http / https 外链',
        retryable: false,
      };
    }
    window.open(trimmed, '_blank', 'noopener,noreferrer');
  }
}
