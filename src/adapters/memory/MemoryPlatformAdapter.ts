import type { PlatformPort } from '../../ports/platformPort';
import { externalMailUrl } from '../../shared/mail-content/mailDocument';

/** 开发环境：在新窗口打开外链 */
export class MemoryPlatformAdapter implements PlatformPort {
  async openExternalUrl(url: string): Promise<void> {
    const trimmed = externalMailUrl(url);
    if (!trimmed) {
      throw {
        code: 'validation',
        message: '仅允许 http / https / mailto 外链',
        retryable: false,
      };
    }
    window.open(trimmed, '_blank', 'noopener,noreferrer');
  }
}
