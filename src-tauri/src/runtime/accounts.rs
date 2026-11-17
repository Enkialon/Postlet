//! 账号变更先隔离并排空收信任务，再调用配置/凭据用例。
use super::mail_sync_gate::MailSyncPause;
use super::mail_watchers::WatcherPause;
use crate::AppState;
use crate::application::dto::UpsertImapSmtpAccountCommand;
use crate::application::error::AppError;
use crate::application::use_cases::{account_use_case, mail_account_use_case};
use crate::domain::entities::Account;
use crate::domain::value_objects::{AccountId, EmailAddress};

async fn quiesce(
    state: &AppState,
    account_id: &AccountId,
) -> Result<(MailSyncPause, WatcherPause), AppError> {
    let gate = state
        .mail_sync_gate
        .pause_and_wait(account_id.as_str())
        .await?;
    let watcher = state.mail_watchers.pause(account_id).await?;
    Ok((gate, watcher))
}

pub async fn sign_in(state: &AppState) -> Result<Account, AppError> {
    let pending = account_use_case::begin_graph_sign_in(&state.paths).await?;
    let existing = account_use_case::graph_sign_in_account(&state.db, &pending).await?;
    let _paused = match &existing {
        Some(id) => Some(quiesce(state, id).await?),
        None => None,
    };
    let account = account_use_case::complete_graph_sign_in(
        &state.db,
        &state.paths,
        pending,
        existing.as_ref(),
    )
    .await?;
    if _paused.is_none() {
        state.mail_watchers.changed(&account.id);
    }
    Ok(account)
}

pub async fn sign_out(state: &AppState, account_id: &AccountId) -> Result<(), AppError> {
    let _paused = quiesce(state, account_id).await?;
    account_use_case::sign_out(&state.db, &state.paths, account_id).await
}

pub async fn clear_local_data(state: &AppState, account_id: &AccountId) -> Result<(), AppError> {
    let _paused = quiesce(state, account_id).await?;
    let changeset =
        account_use_case::clear_local_data(&state.db, &state.paths, account_id).await?;
    state.changes.dispatch(changeset);
    Ok(())
}

pub async fn upsert_imap_smtp_account(
    state: &AppState,
    cmd: UpsertImapSmtpAccountCommand,
) -> Result<Account, AppError> {
    mail_account_use_case::validate(&cmd)?;
    let existing = {
        let mut conn = state.db.acquire().await?;
        let email =
            EmailAddress::parse(&cmd.email).ok_or_else(|| AppError::validation("邮箱地址无效"))?;
        crate::infrastructure::repository::accounts::get_by_email(&mut conn, &email).await?
    };
    let _paused = match existing {
        Some(account) => Some(quiesce(state, &account.id).await?),
        None => None,
    };
    let account =
        mail_account_use_case::upsert_imap_smtp_account(&state.db, &state.paths, cmd).await?;
    // 新账号没有旧任务；已存在账号的 pause guard 会触发重新核对。
    if _paused.is_none() {
        state.mail_watchers.changed(&account.id);
    }
    Ok(account)
}
