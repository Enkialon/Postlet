import type { AppError } from '../../domain/errors';

/** 将未知 IPC / 运行时错误归一为 AppError */
export function normalizeAppError(error: unknown): AppError {
  if (
    error &&
    typeof error === 'object' &&
    'code' in error &&
    'message' in error &&
    'retryable' in error
  ) {
    const e = error as AppError;
    return {
      code: e.code,
      message: String(e.message),
      retryable: Boolean(e.retryable),
    };
  }
  if (error instanceof Error) {
    return {
      code: 'internal',
      message: error.message || '未知错误',
      retryable: true,
    };
  }
  return {
    code: 'internal',
    message: '未知错误',
    retryable: true,
  };
}

export function rejectAsAppError(error: unknown): never {
  throw normalizeAppError(error);
}
