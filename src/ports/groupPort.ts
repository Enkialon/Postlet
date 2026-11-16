import type { Group, GroupParticipant, Session } from '../domain/entities';
import type {
  CreateManualGroupCommand,
  CursorPage,
  GroupListQuery,
  HideGroupCommand,
  MergeSessionsCommand,
  RenameGroupCommand,
  UpdateGroupSendMembersCommand,
} from '../domain/dto';
import type { AccountId, GroupId } from '../domain/types';

/** 群组入站端口 */
export interface GroupPort {
  listGroups(query: GroupListQuery): Promise<CursorPage<Group>>;
  getGroup(accountId: AccountId, groupId: GroupId): Promise<Group>;
  listParticipants(
    accountId: AccountId,
    groupId: GroupId,
  ): Promise<GroupParticipant[]>;
  createManualGroup(cmd: CreateManualGroupCommand): Promise<Group>;
  renameGroup(cmd: RenameGroupCommand): Promise<Group>;
  hideGroup(cmd: HideGroupCommand): Promise<void>;
  /** 合并群聊 Session：source → target */
  mergeSessions(cmd: MergeSessionsCommand): Promise<Session>;
  updateSendMembers(
    cmd: UpdateGroupSendMembersCommand,
  ): Promise<GroupParticipant[]>;
}
