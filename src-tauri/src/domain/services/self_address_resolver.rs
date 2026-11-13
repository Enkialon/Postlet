//! 从账户解析 self 排除集合

use crate::domain::entities::Account;
use crate::domain::value_objects::SelfAddressSet;

/// 主邮箱与别名去重组成 self 排除集合
pub fn resolve_self_addresses(account: &Account) -> SelfAddressSet {
    let mut addresses = Vec::with_capacity(1 + account.aliases.len());
    addresses.push(account.email.clone());
    for alias in &account.aliases {
        if !addresses.iter().any(|a| a == alias) {
            addresses.push(alias.clone());
        }
    }
    SelfAddressSet { addresses }
}
