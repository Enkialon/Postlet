import type { AppError } from '../../domain/errors';

export function asAppError(error: unknown): AppError {
  if (
    error &&
    typeof error === 'object' &&
    'code' in error &&
    'message' in error &&
    'retryable' in error
  ) {
    return error as AppError;
  }
  if (error instanceof Error) {
    return { code: 'internal', message: error.message, retryable: true };
  }
  return { code: 'internal', message: '未知错误', retryable: true };
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
      return '出现错误，请稍后重试';
  }
}
