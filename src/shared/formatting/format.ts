import type { TimestampMs } from '../../domain/types';

/** 本地时区时间展示；缺失返回空串由调用方决定「时间未知」 */
export function formatTimestamp(
  value: TimestampMs | null | undefined,
  locale = 'zh-CN',
): string {
  if (value == null) return '';
  try {
    return new Intl.DateTimeFormat(locale, {
      year: 'numeric',
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    }).format(new Date(value));
  } catch {
    return '';
  }
}

export function formatTimestampOrUnknown(
  value: TimestampMs | null | undefined,
  locale = 'zh-CN',
  unknownLabel = '时间未知',
): string {
  return formatTimestamp(value, locale) || unknownLabel;
}

/** 名字与邮箱相同（忽略大小写）则视为没有昵称 */
function isUsableDisplayName(
  name: string,
  address: string | null | undefined,
): boolean {
  if (!name) return false;
  const a = address?.trim().toLowerCase() || '';
  if (a && name.toLowerCase() === a) return false;
  return true;
}

/**
 * 有昵称用昵称；名字为空或只是邮箱本身时，可用 fallbackName，再不行才用邮箱。
 */
export function displayNameOrAddress(
  name: string | null | undefined,
  address: string | null | undefined,
  fallbackName?: string | null | undefined,
): string {
  const n = name?.trim() || '';
  if (isUsableDisplayName(n, address)) return n;
  const f = fallbackName?.trim() || '';
  if (isUsableDisplayName(f, address)) return f;
  return address?.trim() || '';
}

export function formatUnreadBadge(count: number): string {
  if (count <= 0) return '';
  if (count > 99) return '99+';
  return String(count);
}

export function truncateText(text: string, max = 80): string {
  const t = text.trim();
  if (t.length <= max) return t;
  return `${t.slice(0, max - 1)}…`;
}

/** 会话列表用的短时间（接近微信：今天只显示时分） */
export function formatChatTime(
  value: TimestampMs | null | undefined,
  locale = 'zh-CN',
): string {
  if (value == null) return '';
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return '';
  const now = new Date();
  const startToday = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const startThat = new Date(
    date.getFullYear(),
    date.getMonth(),
    date.getDate(),
  );
  const dayDiff = Math.round(
    (startToday.getTime() - startThat.getTime()) / 86_400_000,
  );
  const hm = new Intl.DateTimeFormat(locale, {
    hour: '2-digit',
    minute: '2-digit',
  }).format(date);
  if (dayDiff === 0) return hm;
  if (dayDiff === 1) return `昨天 ${hm}`;
  if (dayDiff > 1 && dayDiff < 7) {
    return new Intl.DateTimeFormat(locale, { weekday: 'short' }).format(date);
  }
  return new Intl.DateTimeFormat(locale, {
    month: 'numeric',
    day: 'numeric',
  }).format(date);
}
