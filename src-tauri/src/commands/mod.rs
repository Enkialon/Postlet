//! Tauri IPC 薄胶水：参数 → 用例 → DTO；不含 Graph / 业务规则。

use crate::query_page::CursorPage;
use crate::application::dto::{
    ComposeAttachmentDraft, ComposeDraft, ComposeSendResult, ContactListQuery, ContactSyncProgress,
    CreateManualGroupCommand, DeleteMessagesCommand, EnsureGroupSessionCommand, HideGroupCommand,
    EnsurePrivateSessionCommand, GroupListQuery, ImportAttachmentCommand, MailSyncProgress,
    MarkMessagesReadCommand, MergeSessionsCommand, ReassignMessagesCommand, RenameGroupCommand,
    SaveComposeDraftCommand, SendComposedMailCommand, SessionListItem, SessionListQuery,
    SessionMessagesQuery, SetFolderSyncEnabledCommand, TriggerContactSyncCommand,
    TriggerMailSyncCommand, UpdateAccountAliasesCommand, UpdateAccountDisplaySettingsCommand,
    UpdateGroupSendMembersCommand, UpsertImapSmtpAccountCommand,
};
use crate::application::error::AppError;
use crate::application::use_cases::{
    account_use_case, compose_use_case, contact_use_case, group_use_case,
    message_use_case, session_use_case, sync_use_case,
};
use crate::domain::entities::{
    Account, Contact, Group, GroupParticipant, MailFolderSyncState, MessageWithRecipients,
    SendState, Session,
};
use crate::domain::value_objects::{AccountId, ContactId, GroupId, MessageId, SessionId};
use crate::AppState;
use tauri::State;

#[tauri::command]
pub async fn list_accounts(state: State<'_, AppState>) -> Result<Vec<Account>, AppError> {
    account_use_case::list_accounts(&state.db).await
}

#[tauri::command]
pub async fn get_account(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<Account, AppError> {
    account_use_case::get_account(&state.db, &AccountId::new(account_id)).await
}

#[tauri::command]
pub async fn account_sign_in(state: State<'_, AppState>) -> Result<Account, AppError> {
    crate::runtime::accounts::sign_in(&state).await
}

#[tauri::command]
pub async fn account_sign_out(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<(), AppError> {
    crate::runtime::accounts::sign_out(&state, &AccountId::new(account_id)).await
}

#[tauri::command]
pub async fn account_clear_local_data(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<(), AppError> {
    crate::runtime::accounts::clear_local_data(&state, &AccountId::new(account_id)).await
}

#[tauri::command]
pub async fn update_display_settings(
    state: State<'_, AppState>,
    cmd: UpdateAccountDisplaySettingsCommand,
) -> Result<Account, AppError> {
    account_use_case::update_display_settings(&state.db, cmd).await
}

#[tauri::command]
pub async fn update_account_aliases(
    state: State<'_, AppState>,
    cmd: UpdateAccountAliasesCommand,
) -> Result<Account, AppError> {
    let (account, changeset) = account_use_case::update_aliases(&state.db, cmd).await?;
    if let Some(changeset) = changeset {
        state.changes.dispatch(changeset);
    }
    Ok(account)
}

#[tauri::command]
pub async fn get_account_change_version(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<i64, AppError> {
    account_use_case::account_change_version(&state.db, &AccountId::new(account_id)).await
}

#[tauri::command]
pub async fn list_contacts(
    state: State<'_, AppState>,
    query: ContactListQuery,
) -> Result<CursorPage<Contact>, AppError> {
    contact_use_case::list_contacts(&state.db, query).await
}

#[tauri::command]
pub async fn get_contact(
    state: State<'_, AppState>,
    account_id: String,
    contact_id: String,
) -> Result<Contact, AppError> {
    contact_use_case::get_contact(
        &state.db,
        &AccountId::new(account_id),
        &ContactId::new(contact_id),
    )
    .await
}

#[tauri::command]
pub async fn get_contact_by_address(
    state: State<'_, AppState>,
    account_id: String,
    address: String,
) -> Result<Option<Contact>, AppError> {
    let email = crate::domain::value_objects::EmailAddress::parse(&address).ok_or_else(|| {
        AppError::validation("无效邮箱地址")
    })?;
    contact_use_case::get_contact_by_address(&state.db, &AccountId::new(account_id), &email).await
}

#[tauri::command]
pub async fn list_sessions(
    state: State<'_, AppState>,
    query: SessionListQuery,
) -> Result<CursorPage<SessionListItem>, AppError> {
    session_use_case::list_sessions(&state.db, query).await
}

#[tauri::command]
pub async fn get_session(
    state: State<'_, AppState>,
    account_id: String,
    session_id: String,
) -> Result<SessionListItem, AppError> {
    session_use_case::get_session(
        &state.db,
        &AccountId::new(account_id),
        &SessionId::new(session_id),
    )
    .await
}

#[tauri::command]
pub async fn list_session_messages(
    state: State<'_, AppState>,
    query: SessionMessagesQuery,
) -> Result<CursorPage<MessageWithRecipients>, AppError> {
    session_use_case::list_messages(&state.db, query).await
}

#[tauri::command]
pub async fn list_pending_sends(
    state: State<'_, AppState>,
    account_id: String,
    session_id: String,
) -> Result<Vec<MessageWithRecipients>, AppError> {
    session_use_case::list_pending_sends(
        &state.db,
        &crate::domain::value_objects::AccountId::new(account_id),
        &crate::domain::value_objects::SessionId::new(session_id),
    )
    .await
}

#[tauri::command]
pub async fn hide_session(
    state: State<'_, AppState>,
    account_id: String,
    session_id: String,
) -> Result<(), AppError> {
    session_use_case::hide_session(
        &state.db,
        &AccountId::new(account_id),
        &SessionId::new(session_id),
    )
    .await
}

#[tauri::command]
pub async fn get_message(
    state: State<'_, AppState>,
    account_id: String,
    message_id: String,
) -> Result<MessageWithRecipients, AppError> {
    message_use_case::get_message(
        &state.db,
        &AccountId::new(account_id),
        &MessageId::new(message_id),
    )
    .await
}

#[tauri::command]
pub async fn ensure_message_body(
    state: State<'_, AppState>,
    account_id: String,
    message_id: String,
) -> Result<MessageWithRecipients, AppError> {
    message_use_case::ensure_message_body(
        &state.db,
        &state.paths,
        &AccountId::new(account_id),
        &MessageId::new(message_id),
    )
    .await
}

#[tauri::command]
pub async fn mark_messages_read(
    state: State<'_, AppState>,
    cmd: MarkMessagesReadCommand,
) -> Result<(), AppError> {
    let account_id = cmd.account_id.clone();
    let changes = message_use_case::mark_read(&state.db, cmd).await?;
    let flush = changes.pending_remote_flush;
    state.changes.dispatch(changes);
    if flush {
        crate::runtime::mail_sync::spawn_pending_flush(
            state.db.clone(),
            state.paths.clone(),
            account_id,
        );
    }
    Ok(())
}

#[tauri::command]
pub async fn delete_messages(
    state: State<'_, AppState>,
    cmd: DeleteMessagesCommand,
) -> Result<(), AppError> {
    let account_id = cmd.account_id.clone();
    let changes = message_use_case::delete_messages(&state.db, cmd).await?;
    let flush = changes.pending_remote_flush;
    state.changes.dispatch(changes);
    if flush {
        crate::runtime::mail_sync::spawn_pending_flush(
            state.db.clone(),
            state.paths.clone(),
            account_id,
        );
    }
    Ok(())
}

#[tauri::command]
pub async fn list_groups(
    state: State<'_, AppState>,
    query: GroupListQuery,
) -> Result<CursorPage<Group>, AppError> {
    group_use_case::list_groups(&state.db, query).await
}

#[tauri::command]
pub async fn get_group(
    state: State<'_, AppState>,
    account_id: String,
    group_id: String,
) -> Result<Group, AppError> {
    group_use_case::get_group(
        &state.db,
        &AccountId::new(account_id),
        &GroupId::new(group_id),
    )
    .await
}

#[tauri::command]
pub async fn list_group_participants(
    state: State<'_, AppState>,
    account_id: String,
    group_id: String,
) -> Result<Vec<GroupParticipant>, AppError> {
    group_use_case::list_participants(
        &state.db,
        &AccountId::new(account_id),
        &GroupId::new(group_id),
    )
    .await
}

#[tauri::command]
pub async fn get_mail_sync_progress(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<MailSyncProgress, AppError> {
    sync_use_case::get_mail_sync_progress(&state.db, &AccountId::new(account_id)).await
}

#[tauri::command]
pub async fn trigger_mail_sync(
    state: State<'_, AppState>,
    cmd: TriggerMailSyncCommand,
) -> Result<(), AppError> {
    let db = state.db.clone();
    let paths = state.paths.clone();
    let changes = state.changes.clone();
    let gate = state.mail_sync_gate.clone();
    tauri::async_runtime::spawn(async move {
        let _ = crate::runtime::mail_sync::trigger_mail_sync(&db, &paths, &changes, &gate, cmd).await;
    });
    Ok(())
}

#[tauri::command]
pub async fn refresh_mail_folders(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<Vec<MailFolderSyncState>, AppError> {
    crate::runtime::mail_sync::refresh_mail_folders(
        &state.db,
        &state.paths,
        &state.mail_sync_gate,
        &AccountId::new(account_id),
    )
    .await
}

#[tauri::command]
pub async fn set_folder_sync_enabled(
    state: State<'_, AppState>,
    cmd: SetFolderSyncEnabledCommand,
) -> Result<MailFolderSyncState, AppError> {
    sync_use_case::set_folder_sync_enabled(&state.db, cmd).await
}

#[tauri::command]
pub async fn upsert_imap_smtp_account(
    state: State<'_, AppState>,
    cmd: UpsertImapSmtpAccountCommand,
) -> Result<Account, AppError> {
    crate::runtime::accounts::upsert_imap_smtp_account(&state, cmd).await
}

#[tauri::command]
pub async fn trigger_contact_sync(
    state: State<'_, AppState>,
    cmd: TriggerContactSyncCommand,
) -> Result<(), AppError> {
    sync_use_case::trigger_contact_sync(&state.db, &state.paths, cmd).await
}

#[tauri::command]
pub async fn get_contact_sync_progress(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<ContactSyncProgress, AppError> {
    sync_use_case::get_contact_sync_progress(&state.db, &AccountId::new(account_id)).await
}

#[tauri::command]
pub async fn ensure_private_session(
    state: State<'_, AppState>,
    cmd: EnsurePrivateSessionCommand,
) -> Result<Session, AppError> {
    session_use_case::ensure_private_session(&state.db, cmd).await
}

#[tauri::command]
pub async fn ensure_group_session(
    state: State<'_, AppState>,
    cmd: EnsureGroupSessionCommand,
) -> Result<Session, AppError> {
    session_use_case::ensure_group_session(&state.db, cmd).await
}

#[tauri::command]
pub async fn create_manual_group(
    state: State<'_, AppState>,
    cmd: CreateManualGroupCommand,
) -> Result<Group, AppError> {
    group_use_case::create_manual_group(&state.db, cmd).await
}

#[tauri::command]
pub async fn rename_group(
    state: State<'_, AppState>,
    cmd: RenameGroupCommand,
) -> Result<Group, AppError> {
    group_use_case::rename_group(&state.db, cmd).await
}

#[tauri::command]
pub async fn hide_group(
    state: State<'_, AppState>,
    cmd: HideGroupCommand,
) -> Result<(), AppError> {
    group_use_case::hide_group(&state.db, cmd).await
}

#[tauri::command]
pub async fn merge_sessions(
    state: State<'_, AppState>,
    cmd: MergeSessionsCommand,
) -> Result<Session, AppError> {
    group_use_case::merge_group_sessions(&state.db, cmd).await
}

#[tauri::command]
pub async fn reassign_messages(
    state: State<'_, AppState>,
    cmd: ReassignMessagesCommand,
) -> Result<(), AppError> {
    session_use_case::reassign_messages(&state.db, cmd).await
}

#[tauri::command]
pub async fn update_group_send_members(
    state: State<'_, AppState>,
    cmd: UpdateGroupSendMembersCommand,
) -> Result<Vec<GroupParticipant>, AppError> {
    group_use_case::update_send_members(&state.db, cmd).await
}

#[tauri::command]
pub async fn compose_get_draft(
    state: State<'_, AppState>,
    account_id: String,
    session_id: String,
) -> Result<Option<ComposeDraft>, AppError> {
    compose_use_case::get_draft(
        &state.db,
        &AccountId::new(account_id),
        &SessionId::new(session_id),
    )
    .await
}

#[tauri::command]
pub async fn compose_save_draft(
    state: State<'_, AppState>,
    cmd: SaveComposeDraftCommand,
) -> Result<ComposeDraft, AppError> {
    compose_use_case::save_draft(&state.db, &state.paths, cmd).await
}

#[tauri::command]
pub async fn compose_clear_draft(
    state: State<'_, AppState>,
    account_id: String,
    session_id: String,
) -> Result<(), AppError> {
    compose_use_case::clear_draft(
        &state.db,
        &state.paths,
        &AccountId::new(account_id),
        &SessionId::new(session_id),
    )
    .await
}

#[tauri::command]
pub async fn compose_import_attachment(
    state: State<'_, AppState>,
    cmd: ImportAttachmentCommand,
) -> Result<ComposeAttachmentDraft, AppError> {
    compose_use_case::import_attachment(&state.db, &state.paths, cmd).await
}

#[tauri::command]
pub async fn compose_send(
    state: State<'_, AppState>,
    cmd: SendComposedMailCommand,
) -> Result<ComposeSendResult, AppError> {
    let result = compose_use_case::send(&state.db, &state.paths, cmd).await?;
    // 入队即派发一次；失败由 send_worker 的周期 tick 兜底
    crate::runtime::send_worker::kick(
        state.db.clone(),
        state.paths.clone(),
        state.changes.clone(),
        state.mail_sync_gate.clone(),
    );
    Ok(result)
}

#[tauri::command]
pub async fn get_send_state(
    state: State<'_, AppState>,
    account_id: String,
    message_id: String,
) -> Result<Option<SendState>, AppError> {
    let mut conn = state.db.acquire().await?;
    let task = crate::infrastructure::repository::send_tasks::get_for_message(
        &mut conn,
        &AccountId::new(account_id),
        &crate::domain::value_objects::MessageId::new(message_id),
    )
    .await
    .map_err(AppError::from)?;
    Ok(task.map(|t| t.state))
}
