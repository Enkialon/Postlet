//! 会话聚合纯规则：输入完整快照，输出期望投影

use super::message_attribution::{observed_participants, peer_addresses_for_contact_projection};
use crate::domain::entities::MessageWithRecipients;
use crate::domain::error::DomainError;
use crate::domain::value_objects::{AggregationOutcome, EmailAddress, GroupId, SelfAddressSet};

/// 聚合决策上下文
#[derive(Debug, Clone)]
pub struct AggregationContext {
    pub message: MessageWithRecipients,
    pub self_set: SelfAddressSet,
    /// 已解析到存活根群组；现有归属优先于自动推断
    pub thread_group: Option<GroupId>,
    pub show_group_messages_in_contact_conversation: bool,
}

/// 群组目标
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupTarget {
    Existing(GroupId),
    Create { members: Vec<EmailAddress> },
}

/// 期望状态；Application 对比旧附着执行增删，避免重复累计
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AggregationPlan {
    pub contact_addresses: Vec<EmailAddress>,
    pub group_target: Option<GroupTarget>,
    pub observed_participants: Vec<(EmailAddress, String)>,
    pub outcome: AggregationOutcome,
}

/// 纯聚合决策，不访问仓储或持有可替换策略对象
pub fn decide_aggregation(context: &AggregationContext) -> Result<AggregationPlan, DomainError> {
    let peers = peer_addresses_for_contact_projection(&context.message, &context.self_set);
    let observed = observed_participants(&context.message, &context.self_set);

    let has_thread = context.message.message.thread_id.is_some();

    // 仅 self 或无有效对端且无可执行动作 → Skipped
    if peers.is_empty() && context.thread_group.is_none() && !has_thread {
        return Ok(AggregationPlan {
            contact_addresses: Vec::new(),
            group_target: None,
            observed_participants: observed,
            outcome: AggregationOutcome::Skipped,
        });
    }

    // 无 ThreadId：仅做可确定的联系人投影 → Partial
    if !has_thread {
        return Ok(AggregationPlan {
            contact_addresses: peers,
            group_target: None,
            observed_participants: observed,
            outcome: AggregationOutcome::Partial,
        });
    }

    // 已有根群组优先
    let group_target = if let Some(gid) = &context.thread_group {
        Some(GroupTarget::Existing(gid.clone()))
    } else if peers.len() >= 2 {
        Some(GroupTarget::Create {
            members: peers.clone(),
        })
    } else {
        None
    };

    let contact_addresses = match (
        &group_target,
        context.show_group_messages_in_contact_conversation,
    ) {
        (Some(_), false) => Vec::new(),
        _ => peers,
    };

    Ok(AggregationPlan {
        contact_addresses,
        group_target,
        observed_participants: observed,
        outcome: AggregationOutcome::Completed,
    })
}
