import type { ContactPort } from '../../ports/contactPort';
import type { Contact } from '../../domain/entities';
import type { ContactListQuery, CursorPage } from '../../domain/dto';
import type { AccountId, ContactId, EmailAddress } from '../../domain/types';
import type { MemoryStore } from './memoryStore';

function paginate<T>(
  items: T[],
  cursor: string | null,
  limit: number,
): CursorPage<T> {
  const start = cursor ? Number.parseInt(cursor, 10) : 0;
  const safeStart = Number.isFinite(start) && start >= 0 ? start : 0;
  const slice = items.slice(safeStart, safeStart + limit);
  const next = safeStart + slice.length;
  const has_more = next < items.length;
  return {
    items: slice,
    next_cursor: has_more ? String(next) : null,
    has_more,
  };
}

export class MemoryContactAdapter implements ContactPort {
  constructor(private readonly store: MemoryStore) {}

  async listContacts(query: ContactListQuery): Promise<CursorPage<Contact>> {
    this.store.requireAccount(query.account_id);
    const search = query.search?.trim().toLowerCase() ?? null;
    let items = [...this.store.contacts.values()]
      .filter((c) => c.account_id === query.account_id)
      .sort((a, b) =>
        (a.display_name || a.normalized_address).localeCompare(
          b.display_name || b.normalized_address,
        ),
      );
    if (search) {
      items = items.filter(
        (c) =>
          c.display_name.toLowerCase().startsWith(search) ||
          c.normalized_address.startsWith(search),
      );
    }
    return paginate(items, query.cursor, Math.max(1, Math.min(query.limit, 200)));
  }

  async getContact(
    accountId: AccountId,
    contactId: ContactId,
  ): Promise<Contact> {
    this.store.requireAccount(accountId);
    const contact = this.store.contacts.get(
      this.store.contactKey(accountId, contactId),
    );
    if (!contact) this.store.failNotFound('联系人不存在');
    return contact;
  }

  async getContactByAddress(
    accountId: AccountId,
    address: EmailAddress,
  ): Promise<Contact | null> {
    this.store.requireAccount(accountId);
    const normalized = this.store.normalizeEmail(address);
    for (const c of this.store.contacts.values()) {
      if (c.account_id === accountId && c.normalized_address === normalized) {
        return c;
      }
    }
    return null;
  }
}
