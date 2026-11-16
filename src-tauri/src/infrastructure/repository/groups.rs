//! SQLite groups 模块函数；连接与事务由调用方持有。

use crate::application::dto::{CursorPage, GroupListQuery};
use crate::application::error::PortError;
use crate::domain::entities::{Group, GroupParticipant};
use crate::domain::value_objects::{AccountId, GroupId};
use crate::infrastructure::repository::accounts;
use sqlx::{QueryBuilder, Sqlite, SqliteConnection};

pub async fn get(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    id: &GroupId,
) -> Result<Option<Group>, PortError> {
    let row = sqlx::query("SELECT * FROM groups WHERE account_id = ? AND id = ?")
        .bind(account_id.as_str())
        .bind(id.as_str())
        .fetch_optional(&mut *conn)
        .await?;
    row.as_ref()
        .map(super::rows::group)
        .transpose()
        .map_err(Into::into)
}

/// 读取并套用自动群展示名（联系人昵称优先）
pub async fn get_for_display(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    id: &GroupId,
) -> Result<Option<Group>, PortError> {
    let mut group = match get(conn, account_id, id).await? {
        Some(g) => g,
        None => return Ok(None),
    };
    apply_display_name(conn, &mut group).await?;
    Ok(Some(group))
}

pub async fn list(
    conn: &mut SqliteConnection,
    query: &GroupListQuery,
) -> Result<CursorPage<Group>, PortError> {
    if query.limit == 0 || query.limit > 500 {
        return Err(PortError::Conflict {
            message: "分页大小须为 1..500".into(),
        });
    }
    let account_ids = accounts::resolve_list_account_ids(conn, query.account_id.as_ref()).await?;
    if account_ids.is_empty() {
        return Ok(CursorPage::empty());
    }
    let (cursor_account, cursor_id) = parse_entity_cursor(query.cursor.as_deref())?;

    let mut qb = QueryBuilder::new("SELECT * FROM groups WHERE ");
    accounts::push_account_id_in(&mut qb, &account_ids);
    // 群组页 = 真群组；auto 多人往来只出现在会话列表
    qb.push(" AND source = 'manual' AND (");
    if cursor_id.is_none() {
        qb.push("1 = 1");
    } else {
        qb.push("account_id > ");
        qb.push_bind(cursor_account.as_deref().unwrap_or(""));
        qb.push(" OR (account_id = ");
        qb.push_bind(cursor_account.as_deref().unwrap_or(""));
        qb.push(" AND id > ");
        qb.push_bind(cursor_id.as_deref());
        qb.push(")");
    }
    qb.push(") AND (");
    if let Some(search) = query.search.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        // 群名，或成员昵称/联系人昵称/地址前缀
        qb.push("instr(lower(name), lower(");
        qb.push_bind(search);
        qb.push(
            ")) = 1 OR EXISTS (
              SELECT 1 FROM group_participants gp
              LEFT JOIN contacts c
                ON c.account_id = gp.account_id
               AND c.normalized_address = gp.normalized_address
              WHERE gp.account_id = groups.account_id AND gp.group_id = groups.id
                AND (
                  instr(lower(gp.display_name), lower(",
        );
        qb.push_bind(search);
        qb.push(")) = 1 OR instr(gp.normalized_address, lower(");
        qb.push_bind(search);
        qb.push(")) = 1 OR instr(lower(COALESCE(c.display_name, '')), lower(");
        qb.push_bind(search);
        qb.push(")) = 1))",
        );
    } else {
        qb.push("1 = 1");
    }
    // 对应群聊 Session 已隐藏的不出现在群组列表（邮件仍在）
    qb.push(
        ") AND NOT EXISTS (
          SELECT 1 FROM sessions s
          WHERE s.account_id = groups.account_id
            AND s.group_id = groups.id
            AND s.kind = 'group'
            AND s.merged_into_id IS NULL
            AND s.hidden_at IS NOT NULL
        ) ORDER BY account_id ASC, id ASC LIMIT ",
    );
    qb.push_bind(i64::from(query.limit) + 1);

    let rows = qb.build().fetch_all(&mut *conn).await?;
    let has_more = rows.len() > query.limit as usize;
    let mut items = rows
        .iter()
        .take(query.limit as usize)
        .map(super::rows::group)
        .collect::<Result<Vec<_>, _>>()?;
    for g in &mut items {
        apply_display_name(conn, g).await?;
    }
    let next_cursor = if has_more {
        items
            .last()
            .map(|x| format!("{}:{}", x.account_id.as_str(), x.id.as_str()))
    } else {
        None
    };
    Ok(CursorPage {
        items,
        next_cursor,
        has_more,
    })
}

/// 自动群：成员昵称优先（联系人表 > 参与者 display_name > 地址）
pub async fn resolve_auto_title(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    group_id: &GroupId,
) -> Result<String, PortError> {
    let parts = list_participants(conn, account_id, group_id).await?;
    if parts.is_empty() {
        return Ok("群组".into());
    }
    let mut pairs: Vec<(String, String)> = Vec::with_capacity(parts.len());
    for p in &parts {
        let addr = p.normalized_address.as_str();
        let name = if let Some(c) =
            super::contacts::get_by_address(conn, account_id, &p.normalized_address).await?
        {
            if crate::domain::services::usable_person_label(&c.display_name, addr) {
                c.display_name
            } else if crate::domain::services::usable_person_label(&p.display_name, addr) {
                p.display_name.clone()
            } else {
                c.display_name
            }
        } else {
            p.display_name.clone()
        };
        pairs.push((addr.to_owned(), name));
    }
    Ok(crate::domain::services::auto_group_title(
        pairs.iter().map(|(a, n)| (a.as_str(), n.as_str())),
    ))
}

/// 自动群：用成员昵称覆盖展示名（不写库）
async fn apply_display_name(
    conn: &mut SqliteConnection,
    group: &mut Group,
) -> Result<(), PortError> {
    if group.name_is_manual {
        return Ok(());
    }
    group.name = resolve_auto_title(conn, &group.account_id, &group.id).await?;
    Ok(())
}

pub async fn insert(conn: &mut SqliteConnection, group: &Group) -> Result<(), PortError> {
    sqlx::query(
        "INSERT INTO groups (
            id, account_id, name, name_is_manual, source, send_members_is_manual,
            created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(group.id.as_str())
    .bind(group.account_id.as_str())
    .bind(&group.name)
    .bind(group.name_is_manual)
    .bind(group.source.as_str())
    .bind(group.send_members_is_manual)
    .bind(group.created_at)
    .bind(group.updated_at)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn upsert(conn: &mut SqliteConnection, group: &Group) -> Result<(), PortError> {
    sqlx::query(
        "INSERT INTO groups (
            id, account_id, name, name_is_manual, source, send_members_is_manual,
            created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(account_id, id) DO UPDATE SET
            name=excluded.name,
            name_is_manual=excluded.name_is_manual,
            source=excluded.source,
            send_members_is_manual=excluded.send_members_is_manual,
            updated_at=excluded.updated_at",
    )
    .bind(group.id.as_str())
    .bind(group.account_id.as_str())
    .bind(&group.name)
    .bind(group.name_is_manual)
    .bind(group.source.as_str())
    .bind(group.send_members_is_manual)
    .bind(group.created_at)
    .bind(group.updated_at)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn update(conn: &mut SqliteConnection, group: &Group) -> Result<(), PortError> {
    let result = sqlx::query(
        "UPDATE groups SET
            name = ?, name_is_manual = ?, source = ?, send_members_is_manual = ?,
            updated_at = ?
         WHERE account_id = ? AND id = ?",
    )
    .bind(&group.name)
    .bind(group.name_is_manual)
    .bind(group.source.as_str())
    .bind(group.send_members_is_manual)
    .bind(group.updated_at)
    .bind(group.account_id.as_str())
    .bind(group.id.as_str())
    .execute(&mut *conn)
    .await?;
    if result.rows_affected() == 0 {
        return Err(PortError::Conflict {
            message: format!("群组不存在: {}", group.id),
        });
    }
    Ok(())
}

pub async fn list_participants(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    group_id: &GroupId,
) -> Result<Vec<GroupParticipant>, PortError> {
    sqlx::query(
        "SELECT * FROM group_participants WHERE account_id = ? AND group_id = ? ORDER BY normalized_address",
    )
    .bind(account_id.as_str())
    .bind(group_id.as_str())
    .fetch_all(&mut *conn)
    .await?
    .iter()
    .map(super::rows::groupparticipant)
    .collect::<Result<Vec<_>, _>>()
    .map_err(Into::into)
}

/// 成员地址命中给定集合的群 id（别名曾被当成群成员时）。
pub async fn list_group_ids_with_addresses(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    addresses: &[crate::domain::value_objects::EmailAddress],
) -> Result<Vec<GroupId>, PortError> {
    if addresses.is_empty() {
        return Ok(Vec::new());
    }
    let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
        "SELECT DISTINCT group_id FROM group_participants WHERE account_id = ",
    );
    qb.push_bind(account_id.as_str());
    qb.push(" AND normalized_address IN (");
    {
        let mut separated = qb.separated(", ");
        for addr in addresses {
            separated.push_bind(addr.as_str());
        }
    }
    qb.push(")");
    let rows = qb.build_query_scalar::<String>().fetch_all(&mut *conn).await?;
    Ok(rows.into_iter().map(GroupId::new).collect())
}

/// 查找 source=auto 且成员地址集合与 `members` 完全一致的群（顺序无关）
pub async fn find_auto_group_by_exact_members(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    members: &[crate::domain::value_objects::EmailAddress],
) -> Result<Option<GroupId>, PortError> {
    if members.is_empty() {
        return Ok(None);
    }
    let mut want: Vec<String> = members.iter().map(|a| a.as_str().to_string()).collect();
    want.sort();
    want.dedup();

    let candidates = sqlx::query(
        "SELECT id FROM groups WHERE account_id = ? AND source = 'auto' ORDER BY created_at ASC",
    )
    .bind(account_id.as_str())
    .fetch_all(&mut *conn)
    .await?;

    for row in candidates {
        use sqlx::Row;
        let gid = GroupId::new(row.try_get::<String, _>("id")?);
        let parts = list_participants(conn, account_id, &gid).await?;
        let mut have: Vec<String> = parts
            .iter()
            .map(|p| p.normalized_address.as_str().to_string())
            .collect();
        have.sort();
        have.dedup();
        if have == want {
            return Ok(Some(gid));
        }
    }
    Ok(None)
}

pub async fn upsert_participants(
    conn: &mut SqliteConnection,
    participants: &[GroupParticipant],
) -> Result<(), PortError> {
    for p in participants {
        sqlx::query(
            "INSERT INTO group_participants (
                account_id, group_id, normalized_address, display_name,
                is_observed, is_send_member, first_seen_at, last_seen_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(account_id, group_id, normalized_address) DO UPDATE SET
                display_name=excluded.display_name,
                is_observed=excluded.is_observed,
                is_send_member=excluded.is_send_member,
                first_seen_at=excluded.first_seen_at,
                last_seen_at=excluded.last_seen_at",
        )
        .bind(p.account_id.as_str())
        .bind(p.group_id.as_str())
        .bind(p.normalized_address.as_str())
        .bind(&p.display_name)
        .bind(p.is_observed)
        .bind(p.is_send_member)
        .bind(p.first_seen_at)
        .bind(p.last_seen_at)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

pub async fn replace_send_members(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    group_id: &GroupId,
    members: &[GroupParticipant],
    send_members_is_manual: bool,
) -> Result<(), PortError> {
    sqlx::query(
        "UPDATE group_participants SET is_send_member = 0
         WHERE account_id = ? AND group_id = ?",
    )
    .bind(account_id.as_str())
    .bind(group_id.as_str())
    .execute(&mut *conn)
    .await?;
    upsert_participants(conn, members).await?;
    sqlx::query(
        "UPDATE groups SET send_members_is_manual = ?, updated_at = ?
         WHERE account_id = ? AND id = ?",
    )
    .bind(send_members_is_manual)
    .bind(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0),
    )
    .bind(account_id.as_str())
    .bind(group_id.as_str())
    .execute(&mut *conn)
    .await?;
    Ok(())
}

fn parse_entity_cursor(
    cursor: Option<&str>,
) -> Result<(Option<String>, Option<String>), PortError> {
    let Some(raw) = cursor else {
        return Ok((None, None));
    };
    if let Some((account_id, id)) = raw.split_once(':') {
        Ok((Some(account_id.to_owned()), Some(id.to_owned())))
    } else {
        Ok((None, Some(raw.to_owned())))
    }
}
