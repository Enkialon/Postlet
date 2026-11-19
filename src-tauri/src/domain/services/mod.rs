//! 纯领域规则（无 IO）：普通函数与数据结构。

mod folder_role;
mod message_attribution;
mod imap_sync_policy;
mod self_address_resolver;
mod session_aggregation_policy;

pub use message_attribution::{
    is_outbound, is_system_mailer_address, observed_participants,
    peer_addresses_for_contact_projection,
};
pub use self_address_resolver::resolve_self_addresses;
pub use folder_role::{
    effective_role, resolve_role, role_from_display_name, role_from_special_use, FolderRole,
    RoleSource,
};
pub use imap_sync_policy::{imap_pending_boundary, missing_imap_page};
pub use session_aggregation_policy::{
    auto_group_title, decide_aggregation, person_label, session_display_name, usable_person_label,
    AggregationContext, AggregationPlan, GroupTarget, ParentSessionHint, SessionTarget,
};
