//! 读路径：按当前 self 集合现算 is_outbound（不以库内缓存列为真相）。

use crate::domain::entities::{Message, MessageWithRecipients};
use crate::domain::services::is_outbound as domain_is_outbound;
use crate::domain::value_objects::SelfAddressSet;
use crate::application::dto::MessagePreview;

/// 覆盖单封邮件的出站标记。
pub fn apply_outbound_flag(message: &mut Message, self_set: &SelfAddressSet) {
    message.is_outbound = domain_is_outbound(message, self_set);
}

/// 覆盖带收件人的邮件包。
pub fn apply_outbound_to_bundle(bundle: &mut MessageWithRecipients, self_set: &SelfAddressSet) {
    apply_outbound_flag(&mut bundle.message, self_set);
}

/// 覆盖会话列表预览。
pub fn apply_outbound_to_preview(preview: &mut MessagePreview, self_set: &SelfAddressSet) {
    preview.is_outbound = preview
        .from_address
        .as_ref()
        .is_some_and(|addr| self_set.contains(addr));
}
