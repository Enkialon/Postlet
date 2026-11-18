//! 领域实体

mod account;
mod aggregate_job;
mod contact;
mod group;
mod mail_transport;
mod message;
mod send_task;
mod session;
mod sync_state;

pub use account::Account;
pub use aggregate_job::SessionAggregateJob;
pub use contact::Contact;
pub use group::{Group, GroupParticipant};
pub use mail_transport::{AccountMailTransport, ReceiveConfig, SendConfig, MailEndpoint, PasswordAuthConfig, GraphAuthConfig, PasswordMailConfig};
pub use message::{Message, MessageRecipient, MessageWithRecipients};
pub use send_task::{backoff_ms, next_state, recover_state, SendState, SendTask};
pub use session::Session;
pub use sync_state::MailFolderSyncState;

mod message_mutation;
pub use message_mutation::{MutationPhase, PendingMessageMutation, PendingMutationKind};
