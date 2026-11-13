import type { Contact } from '../domain/entities';
import type { ContactListQuery, CursorPage } from '../domain/dto';
import type { AccountId, ContactId, EmailAddress } from '../domain/types';

/** 联系人入站端口 */
export interface ContactPort {
  listContacts(query: ContactListQuery): Promise<CursorPage<Contact>>;
  getContact(accountId: AccountId, contactId: ContactId): Promise<Contact>;
  getContactByAddress(
    accountId: AccountId,
    address: EmailAddress,
  ): Promise<Contact | null>;
}
