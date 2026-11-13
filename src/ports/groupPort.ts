import type { Group, GroupParticipant } from '../domain/entities';
import type {
  CreateManualGroupCommand,
  CursorPage,
  GroupListQuery,
  MergeGroupsCommand,
  MoveThreadToGroupCommand,
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
  mergeGroups(cmd: MergeGroupsCommand): Promise<Group>;
  moveThreadToGroup(cmd: MoveThreadToGroupCommand): Promise<void>;
  updateSendMembers(
    cmd: UpdateGroupSendMembersCommand,
  ): Promise<GroupParticipant[]>;
}
