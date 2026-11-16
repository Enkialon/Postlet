import type { Group, GroupParticipant, Session } from '../domain/entities';
import type {
  CreateManualGroupCommand,
  CursorPage,
  GroupListQuery,
  MergeSessionsCommand,
  ReassignMessagesToGroupCommand,
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
  /** 合并群聊 Session：source → target */
  mergeSessions(cmd: MergeSessionsCommand): Promise<Session>;
  /** 将选中消息改挂到目标群聊 Session */
  reassignMessagesToGroup(cmd: ReassignMessagesToGroupCommand): Promise<void>;
  updateSendMembers(
    cmd: UpdateGroupSendMembersCommand,
  ): Promise<GroupParticipant[]>;
}
