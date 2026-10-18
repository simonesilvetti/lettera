// Thin typed wrappers over the Tauri commands and events.

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type {
  AccountInfo,
  Addr,
  AppInfo,
  AttachmentInfo,
  BodyView,
  Draft,
  DraftKind,
  FolderInfo,
  NewAccount,
  SyncStatus,
  ThemePayload,
  ThreadPage,
  ThreadView,
} from './types';

export const api = {
  listAccounts: () => invoke<AccountInfo[]>('list_accounts'),
  addAccount: (account: NewAccount) => invoke<void>('add_account', { account }),
  removeAccount: (email: string) => invoke<void>('remove_account', { email }),
  listFolders: (account: string) => invoke<FolderInfo[]>('list_folders', { account }),
  listThreads: (folder: number, query: string, offset: number, limit: number) =>
    invoke<ThreadPage>('list_threads', { folder, query, offset, limit }),
  getThread: (thread_id: string) => invoke<ThreadView>('get_thread', { thread_id }),
  getBody: (message_id: number, mode: 'text' | 'html') =>
    invoke<BodyView>('get_body', { message_id, mode }),
  setFlags: (message_ids: number[], add: string[], remove: string[]) =>
    invoke<void>('set_flags', { message_ids, add, remove }),
  archive: (message_ids: number[]) => invoke<void>('archive', { message_ids }),
  delete: (message_ids: number[]) => invoke<void>('delete', { message_ids }),
  moveTo: (message_ids: number[], folder: number) =>
    invoke<void>('move_to', { message_ids, folder }),
  prepareDraft: (account: string, kind: DraftKind, source: number | null) =>
    invoke<Draft>('prepare_draft', { account, kind, source }),
  saveDraft: (draft: Draft) => invoke<number>('save_draft', { draft }),
  discardDraft: (id: number) => invoke<void>('discard_draft', { id }),
  send: (draft: Draft) => invoke<void>('send', { draft }),
  listAttachments: (message_id: number) =>
    invoke<AttachmentInfo[]>('list_attachments', { message_id }),
  saveAttachment: (attachment_id: number, path: string | null = null) =>
    invoke<string | null>('save_attachment', { attachment_id, path }),
  pickFiles: () => invoke<string[]>('pick_files'),
  syncNow: (account: string | null) => invoke<void>('sync_now', { account }),
  getTheme: () => invoke<ThemePayload>('get_theme'),
  openConfig: () => invoke<string>('open_config'),
  openUrl: (url: string) => invoke<void>('open_url', { url }),
  startGoogleAuth: (account: NewAccount) => invoke<void>('start_google_auth', { account }),
  enableImages: (message_id: number) => invoke<void>('enable_images', { message_id }),
  suggestContacts: (query: string) => invoke<Addr[]>('suggest_contacts', { query }),
  appInfo: () => invoke<AppInfo>('app_info'),
};

export interface Events {
  'messages:changed': { account: string; folder: number };
  'sync:status': SyncStatus;
  'theme:changed': ThemePayload;
  new_mail: { from: string; subject: string };
  'auth:done': { email: string; ok: boolean; error: string | null };
  'body:ready': { id: number };
  'accounts:changed': Record<string, never>;
}

export function on<K extends keyof Events>(
  name: K,
  handler: (payload: Events[K]) => void,
): Promise<UnlistenFn> {
  return listen<Events[K]>(name, (e) => handler(e.payload));
}
