//! 纯领域规则（无 IO）：普通函数与数据结构。

mod conversation_aggregation_policy;
mod message_attribution;
mod self_address_resolver;

pub use conversation_aggregation_policy::{
    AggregationContext, AggregationPlan, GroupTarget, decide_aggregation,
};
pub use message_attribution::{
    is_outbound, observed_participants, peer_addresses_for_contact_projection,
};
pub use self_address_resolver::resolve_self_addresses;
