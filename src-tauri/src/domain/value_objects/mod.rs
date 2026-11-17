//! 值对象与枚举

mod email;
mod enums;
mod ids;

pub use email::{EmailAddress, SelfAddressSet};
pub use enums::{
    AggregateJobStatus, AggregationOutcome, BodyContentType, ContactSource, GroupSource, Importance,
    MailAuthKind, MailTlsMode, MessageSyncStatus, PageDirection, ReceiveProtocol, RecipientRole,
    SendProtocol, SessionKind, SyncStatus, TimestampMs,
};
pub use ids::{
    AccountId, ContactId, FolderId, RemoteKey, GroupId, MessageId, SessionId,
};
