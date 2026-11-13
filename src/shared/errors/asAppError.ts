import type { AppError } from '../../domain/errors';
import { normalizeAppError } from './normalizeAppError';

export function asAppError(error: unknown): AppError {
  return normalizeAppError(error);
}

export function errorMessageForUi(error: AppError): string {
  switch (error.code) {
    case 'auth':
      return '需要重新登录';
    case 'network':
      return '网络不可用';
    case 'not_found':
      return '内容已不可用';
    case 'validation':
      return error.message || '输入无效';
    case 'conflict':
      return '发生冲突，请刷新后重试';
    case 'sync':
      return '同步出现问题';
    default:
      // 优先展示后端/IPC 原文，避免一律「请稍后重试」
      return error.message || '出现错误，请稍后重试';
  }
}
