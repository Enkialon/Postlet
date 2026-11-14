//! Tauri IPC 薄胶水：参数 → 用例 → DTO；不含 Graph / 业务规则。

use crate::application::dto::{
    ComposeDraft, ContactListQuery, ContactSyncProgress, ConversationListItem,
    ConversationListQuery, ConversationMessagesQuery, CreateManualGroupCommand, CursorPage,
    EnsureContactConversationCommand, EnsureGroupConversationCommand, GroupListQuery,
    MailSyncProgress, MarkMessagesReadCommand, MergeGroupsCommand, MoveThreadToGroupCommand,
    RenameGroupCommand, SaveComposeDraftCommand, SendComposedMailCommand,
    TriggerContactSyncCommand, TriggerMailSyncCommand, UpdateAccountDisplaySettingsCommand,
    UpdateGroupSendMembersCommand, UpsertImapSmtpAccountCommand,
};
use crate::application::error::AppError;
use crate::application::use_cases::{
    account_use_case, compose_use_case, contact_use_case, conversation_use_case, group_use_case,
    mail_account_use_case, message_use_case, sync_use_case,
};
use crate::domain::entities::{
    Account, Contact, Conversation, Group, GroupParticipant, MessageWithRecipients,
};
use crate::domain::value_objects::{AccountId, ContactId, ConversationId, GroupId, MessageId};
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
    account_use_case::sign_in(&state.db, &state.paths).await
}

#[tauri::command]
pub async fn account_sign_out(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<(), AppError> {
    account_use_case::sign_out(&state.db, &state.paths, &AccountId::new(account_id)).await
}

#[tauri::command]
pub async fn load_demo_workspace(state: State<'_, AppState>) -> Result<Account, AppError> {
    account_use_case::load_demo_workspace(&state.db).await
}

#[tauri::command]
pub async fn update_display_settings(
    state: State<'_, AppState>,
    cmd: UpdateAccountDisplaySettingsCommand,
) -> Result<Account, AppError> {
    account_use_case::update_display_settings(&state.db, cmd).await
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
pub async fn list_conversations(
    state: State<'_, AppState>,
    query: ConversationListQuery,
) -> Result<CursorPage<ConversationListItem>, AppError> {
    conversation_use_case::list_conversations(&state.db, query).await
}

#[tauri::command]
pub async fn get_conversation(
    state: State<'_, AppState>,
    account_id: String,
    conversation_id: String,
) -> Result<ConversationListItem, AppError> {
    conversation_use_case::get_conversation(
        &state.db,
        &AccountId::new(account_id),
        &ConversationId::new(conversation_id),
    )
    .await
}

#[tauri::command]
pub async fn list_conversation_messages(
    state: State<'_, AppState>,
    query: ConversationMessagesQuery,
) -> Result<CursorPage<MessageWithRecipients>, AppError> {
    conversation_use_case::list_messages(&state.db, query).await
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
    let changes = message_use_case::mark_read(&state.db, &state.paths, cmd).await?;
    state.changes.dispatch(changes);
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
    sync_use_case::trigger_mail_sync(&state.db, &state.paths, &state.changes, cmd).await
}

#[tauri::command]
pub async fn upsert_imap_smtp_account(
    state: State<'_, AppState>,
    cmd: UpsertImapSmtpAccountCommand,
) -> Result<Account, AppError> {
    mail_account_use_case::upsert_imap_smtp_account(&state.db, &state.paths, cmd).await
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
pub async fn ensure_contact_conversation(
    state: State<'_, AppState>,
    cmd: EnsureContactConversationCommand,
) -> Result<Conversation, AppError> {
    conversation_use_case::ensure_contact_conversation(&state.db, cmd).await
}

#[tauri::command]
pub async fn ensure_group_conversation(
    state: State<'_, AppState>,
    cmd: EnsureGroupConversationCommand,
) -> Result<Conversation, AppError> {
    conversation_use_case::ensure_group_conversation(&state.db, cmd).await
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
pub async fn merge_groups(
    state: State<'_, AppState>,
    cmd: MergeGroupsCommand,
) -> Result<Group, AppError> {
    group_use_case::merge_groups(&state.db, cmd).await
}

#[tauri::command]
pub async fn move_thread_to_group(
    state: State<'_, AppState>,
    cmd: MoveThreadToGroupCommand,
) -> Result<(), AppError> {
    group_use_case::move_thread_to_group(&state.db, cmd).await
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
    conversation_id: String,
) -> Result<Option<ComposeDraft>, AppError> {
    compose_use_case::get_draft(
        &state,
        &AccountId::new(account_id),
        &ConversationId::new(conversation_id),
    )
}

#[tauri::command]
pub async fn compose_save_draft(
    state: State<'_, AppState>,
    cmd: SaveComposeDraftCommand,
) -> Result<ComposeDraft, AppError> {
    compose_use_case::save_draft(&state, cmd).await
}

#[tauri::command]
pub async fn compose_clear_draft(
    state: State<'_, AppState>,
    account_id: String,
    conversation_id: String,
) -> Result<(), AppError> {
    compose_use_case::clear_draft(
        &state,
        &AccountId::new(account_id),
        &ConversationId::new(conversation_id),
    )
}

#[tauri::command]
pub async fn compose_send(
    state: State<'_, AppState>,
    cmd: SendComposedMailCommand,
) -> Result<MessageWithRecipients, AppError> {
    compose_use_case::send(&state, cmd).await
}
