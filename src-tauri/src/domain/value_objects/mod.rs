//! 值对象与枚举

mod email;
mod enums;
mod ids;

pub use email::{EmailAddress, SelfAddressSet};
pub use enums::{
    AggregationOutcome, AggregateJobStatus, BodyContentType, ConversationKind, GroupSource,
    Importance, MailAuthKind, MailTlsMode, MessageSyncStatus, PageDirection, ReceiveProtocol,
    RecipientRole, SendProtocol, SyncStatus, TimestampMs,
};
pub use ids::{
    AccountId, ContactId, ConversationId, FolderId, GraphImmutableId, GroupId, MessageId,
    ThreadId,
};
