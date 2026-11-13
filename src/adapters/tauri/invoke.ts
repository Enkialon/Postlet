import { invoke as tauriInvoke } from '@tauri-apps/api/core';
import { normalizeAppError } from './normalizeError';

/** 统一 invoke：失败归一为 AppError */
export async function invoke<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  try {
    return await tauriInvoke<T>(command, args);
  } catch (error) {
    throw normalizeAppError(error);
  }
}

/** 尚未实现的 Port 方法 */
export function notImplemented(feature: string): Promise<never> {
  throw normalizeAppError({
    code: 'internal',
    message: `尚未接入: ${feature}`,
    retryable: false,
  });
}
