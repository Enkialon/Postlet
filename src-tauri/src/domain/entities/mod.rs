//! 领域实体

mod account;
mod aggregate_job;
mod contact;
mod conversation;
mod group;
mod message;
mod sync_state;

pub use account::Account;
pub use aggregate_job::ConversationAggregateJob;
pub use contact::Contact;
pub use conversation::{Conversation, ConversationMessageLink};
pub use group::{Group, GroupParticipant, GroupThread};
pub use message::{Message, MessageRecipient, MessageWithRecipients};
pub use sync_state::MailFolderSyncState;
