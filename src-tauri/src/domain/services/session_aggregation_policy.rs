//! 会话聚合纯规则：一封信只进一个 Session

use super::message_attribution::{observed_participants, peer_addresses_for_contact_projection};
use crate::domain::entities::MessageWithRecipients;
use crate::domain::error::DomainError;
use crate::domain::value_objects::{
    AggregationOutcome, EmailAddress, GroupId, SelfAddressSet, SessionId, SessionKind,
};

/// 聚合决策上下文
#[derive(Debug, Clone)]
pub struct AggregationContext {
    pub message: MessageWithRecipients,
    pub self_set: SelfAddressSet,
    /// 由 In-Reply-To 解析到的父信 Session（已 resolve_root）
    pub parent_session_id: Option<SessionId>,
}

/// 群组目标（仅在需要新建/绑群聊 Session 时）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupTarget {
    Existing(GroupId),
    Create { members: Vec<EmailAddress> },
}

/// 期望唯一 Session
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionTarget {
    /// 沿用父信 / 已有 Session
    Existing(SessionId),
    /// 私聊：按对端地址 ensure
    Private { address: EmailAddress },
    /// 群聊：绑或建 Group + group Session
    Group { target: GroupTarget },
}

/// 期望状态；Application 写入 messages.session_id 并维护群成员
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AggregationPlan {
    pub session_target: Option<SessionTarget>,
    pub observed_participants: Vec<(EmailAddress, String)>,
    pub outcome: AggregationOutcome,
}

/// 纯聚合决策：父信优先，否则按对端人数建私聊/群聊
pub fn decide_aggregation(context: &AggregationContext) -> Result<AggregationPlan, DomainError> {
    let peers = peer_addresses_for_contact_projection(&context.message, &context.self_set);
    let observed = observed_participants(&context.message, &context.self_set);

    if let Some(sid) = &context.parent_session_id {
        return Ok(AggregationPlan {
            session_target: Some(SessionTarget::Existing(sid.clone())),
            observed_participants: observed,
            outcome: AggregationOutcome::Completed,
        });
    }

    if peers.is_empty() {
        return Ok(AggregationPlan {
            session_target: None,
            observed_participants: observed,
            outcome: AggregationOutcome::Skipped,
        });
    }

    if peers.len() == 1 {
        return Ok(AggregationPlan {
            session_target: Some(SessionTarget::Private {
                address: peers[0].clone(),
            }),
            observed_participants: observed,
            outcome: AggregationOutcome::Completed,
        });
    }

    Ok(AggregationPlan {
        session_target: Some(SessionTarget::Group {
            target: GroupTarget::Create {
                members: peers.clone(),
            },
        }),
        observed_participants: observed,
        outcome: AggregationOutcome::Completed,
    })
}

/// 列表显示名：私聊用联系人名（或地址），群聊用群名
pub fn session_display_name(
    kind: SessionKind,
    contact_display_name: Option<&str>,
    private_address: Option<&str>,
    group_name: Option<&str>,
) -> String {
    match kind {
        SessionKind::Private => {
            let name = contact_display_name.unwrap_or("").trim();
            if !name.is_empty() {
                name.to_owned()
            } else {
                private_address.unwrap_or("").to_owned()
            }
        }
        SessionKind::Group => group_name.unwrap_or("").to_owned(),
    }
}
