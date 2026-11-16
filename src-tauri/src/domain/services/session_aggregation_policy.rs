//! 会话聚合纯规则：一封信只进一个 Session

use super::message_attribution::{
    is_system_mailer_address, observed_participants, peer_addresses_for_contact_projection,
};
use crate::domain::entities::MessageWithRecipients;
use crate::domain::error::DomainError;
use crate::domain::value_objects::{
    AggregationOutcome, EmailAddress, GroupId, SelfAddressSet, SessionId, SessionKind,
};

/// 父信所在会话（已 resolve_root），仅作兼容性参考，不能无条件覆盖对端人数决策
#[derive(Debug, Clone)]
pub struct ParentSessionHint {
    pub id: SessionId,
    pub kind: SessionKind,
    pub normalized_address: Option<EmailAddress>,
    pub group_id: Option<GroupId>,
    /// 父群当前成员地址集合（私聊为空）
    pub group_member_addresses: Vec<EmailAddress>,
}

/// 聚合决策上下文
#[derive(Debug, Clone)]
pub struct AggregationContext {
    pub message: MessageWithRecipients,
    pub self_set: SelfAddressSet,
    pub parent: Option<ParentSessionHint>,
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
    /// 沿用父信 Session（仅当与对端集合兼容）
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

fn same_address_set(a: &[EmailAddress], b: &[EmailAddress]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().all(|x| b.iter().any(|y| y == x))
}

/// 纯聚合决策：按对端人数定私聊/群聊；父信仅在成员集合兼容时复用
pub fn decide_aggregation(context: &AggregationContext) -> Result<AggregationPlan, DomainError> {
    let peers = peer_addresses_for_contact_projection(&context.message, &context.self_set);
    let observed = observed_participants(&context.message, &context.self_set);

    // DSN / 退信：From 为系统投递员且能定位原信 Session → 直接挂回
    if let Some(parent) = &context.parent {
        let from_is_mailer = context
            .message
            .message
            .from_address
            .as_ref()
            .is_some_and(is_system_mailer_address);
        if from_is_mailer {
            return Ok(AggregationPlan {
                session_target: Some(SessionTarget::Existing(parent.id.clone())),
                observed_participants: observed,
                outcome: AggregationOutcome::Completed,
            });
        }
    }

    if peers.is_empty() {
        return Ok(AggregationPlan {
            session_target: None,
            observed_participants: observed,
            outcome: AggregationOutcome::Skipped,
        });
    }

    if peers.len() == 1 {
        let address = peers[0].clone();
        // 父信为同一私聊 → 复用；否则按地址 ensure（勿把多人往来硬塞进父私聊）
        if let Some(parent) = &context.parent {
            if parent.kind == SessionKind::Private
                && parent
                    .normalized_address
                    .as_ref()
                    .is_some_and(|a| a == &address)
            {
                return Ok(AggregationPlan {
                    session_target: Some(SessionTarget::Existing(parent.id.clone())),
                    observed_participants: observed,
                    outcome: AggregationOutcome::Completed,
                });
            }
        }
        return Ok(AggregationPlan {
            session_target: Some(SessionTarget::Private { address }),
            observed_participants: observed,
            outcome: AggregationOutcome::Completed,
        });
    }

    // 多人 → 群聊；仅当父群成员集合完全一致时沿用父会话
    if let Some(parent) = &context.parent {
        if parent.kind == SessionKind::Group {
            if let Some(gid) = &parent.group_id {
                if same_address_set(&peers, &parent.group_member_addresses) {
                    return Ok(AggregationPlan {
                        session_target: Some(SessionTarget::Existing(parent.id.clone())),
                        observed_participants: observed,
                        outcome: AggregationOutcome::Completed,
                    });
                }
                // 成员不完全一致：仍可提示 Existing group id 给应用层做「同成员复用」之外的扩展；
                // 这里按 peers 建/找群，避免把新成员组合并进旧群。
                let _ = gid;
            }
        }
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

/// 名称可用：非空且不等于邮箱本身
pub fn usable_person_label(name: &str, address: &str) -> bool {
    let n = name.trim();
    if n.is_empty() {
        return false;
    }
    let a = address.trim();
    if a.is_empty() {
        return true;
    }
    !n.eq_ignore_ascii_case(a)
}

/// 单人展示：优先昵称，否则邮箱
pub fn person_label(name: &str, address: &str) -> String {
    if usable_person_label(name, address) {
        name.trim().to_owned()
    } else {
        address.trim().to_owned()
    }
}

/// 自动群标题：成员昵称优先，缺昵称才用邮箱；最多 3 人
pub fn auto_group_title<'a, I>(members: I) -> String
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    let labels: Vec<String> = members
        .into_iter()
        .take(3)
        .map(|(addr, name)| person_label(name, addr))
        .filter(|s| !s.is_empty())
        .collect();
    if labels.is_empty() {
        "群组".into()
    } else {
        labels.join(", ")
    }
}

/// 列表显示名：私聊优先联系人名；群聊用群名（自动群应已是成员昵称拼接）
pub fn session_display_name(
    kind: SessionKind,
    contact_display_name: Option<&str>,
    private_address: Option<&str>,
    group_name: Option<&str>,
) -> String {
    match kind {
        SessionKind::Private => {
            let addr = private_address.unwrap_or("");
            person_label(contact_display_name.unwrap_or(""), addr)
        }
        SessionKind::Group => {
            let name = group_name.unwrap_or("").trim();
            if name.is_empty() {
                "未命名群组".into()
            } else {
                name.to_owned()
            }
        }
    }
}
