//! 纯领域规则（无 IO）：普通函数与数据结构。

mod message_attribution;
mod self_address_resolver;
mod session_aggregation_policy;

pub use message_attribution::{
    is_outbound, observed_participants, peer_addresses_for_contact_projection,
};
pub use self_address_resolver::resolve_self_addresses;
pub use session_aggregation_policy::{
    auto_group_title, decide_aggregation, person_label, session_display_name, usable_person_label,
    AggregationContext, AggregationPlan, GroupTarget, SessionTarget,
};
