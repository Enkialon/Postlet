//! 出站判定与投影地址纯规则

use crate::domain::entities::{Message, MessageWithRecipients};
use crate::domain::value_objects::{EmailAddress, RecipientRole, SelfAddressSet};

pub fn is_outbound(message: &Message, self_set: &SelfAddressSet) -> bool {
    message
        .from_address
        .as_ref()
        .is_some_and(|addr| self_set.contains(addr))
}

pub fn peer_addresses_for_contact_projection(
    message: &MessageWithRecipients,
    self_set: &SelfAddressSet,
) -> Vec<EmailAddress> {
    let mut peers = Vec::new();
    let push_unique = |list: &mut Vec<EmailAddress>, addr: &EmailAddress| {
        if !self_set.contains(addr) && !list.iter().any(|a| a == addr) {
            list.push(addr.clone());
        }
    };

    if let Some(from) = &message.message.from_address {
        push_unique(&mut peers, from);
    }

    for r in &message.recipients {
        match r.role {
            RecipientRole::To | RecipientRole::Cc | RecipientRole::ReplyTo => {
                push_unique(&mut peers, &r.normalized_address);
            }
            RecipientRole::Bcc => {}
        }
    }

    peers
}

pub fn observed_participants(
    message: &MessageWithRecipients,
    self_set: &SelfAddressSet,
) -> Vec<(EmailAddress, String)> {
    let mut out = Vec::new();
    let push = |list: &mut Vec<(EmailAddress, String)>, addr: &EmailAddress, name: &str| {
        if self_set.contains(addr) {
            return;
        }
        if list.iter().any(|(a, _)| a == addr) {
            return;
        }
        list.push((addr.clone(), name.to_owned()));
    };

    if let Some(from) = &message.message.from_address {
        push(&mut out, from, &message.message.from_name);
    }
    for r in &message.recipients {
        match r.role {
            RecipientRole::To | RecipientRole::Cc | RecipientRole::ReplyTo => {
                push(&mut out, &r.normalized_address, &r.display_name);
            }
            RecipientRole::Bcc => {}
        }
    }
    out
}
