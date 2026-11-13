import type { AppError } from '../../domain/errors';

function asAppErrorShape(value: unknown): AppError | null {
  if (
    value &&
    typeof value === 'object' &&
    'code' in value &&
    'message' in value &&
    'retryable' in value
  ) {
    const e = value as AppError;
    return {
      code: e.code,
      message: String(e.message),
      retryable: Boolean(e.retryable),
    };
  }
  return null;
}

function tryParseJson(raw: string): unknown {
  try {
    return JSON.parse(raw);
  } catch {
    return null;
  }
}

/** 将未知 IPC / 运行时错误归一为 AppError */
export function normalizeAppError(error: unknown): AppError {
  const direct = asAppErrorShape(error);
  if (direct) return direct;

  if (typeof error === 'string' && error.trim()) {
    return {
      code: 'internal',
      message: error.trim(),
      retryable: true,
    };
  }

  // Tauri 偶发把 payload 包在 data / error 里；ACL 拒绝常为 { message: string }
  if (error && typeof error === 'object') {
    const obj = error as Record<string, unknown>;
    const nested =
      asAppErrorShape(obj.data) ??
      asAppErrorShape(obj.error) ??
      (typeof obj.message === 'string'
        ? asAppErrorShape(tryParseJson(obj.message))
        : null);
    if (nested) return nested;
    if (typeof obj.message === 'string' && obj.message.trim()) {
      return {
        code: 'internal',
        message: obj.message.trim(),
        retryable: true,
      };
    }
  }

  if (error instanceof Error) {
    const parsed = tryParseJson(error.message);
    const fromMsg = asAppErrorShape(parsed);
    if (fromMsg) return fromMsg;
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
