import { invoke } from "@tauri-apps/api/core"

export interface SessionInfo {
  server_url: string
  user_email: string
  user_name: string
  user_id: string
}

export const commands = {
  login: (serverUrl: string, email: string, password: string) =>
    invoke<SessionInfo>("cmd_login", { serverUrl, email, password }),

  logout: () => invoke<void>("cmd_logout"),

  restoreSession: () => invoke<SessionInfo | null>("cmd_restore_session"),

  uploadFile: (localPath: string, remotePath: string, transferId: string) =>
    invoke<void>("cmd_upload_file", { localPath, remotePath, transferId }),

  listFiles: (path: string) =>
    invoke<unknown>("cmd_list_files", { path }),

  downloadFile: (remotePath: string, localPath: string, transferId: string) =>
    invoke<void>("cmd_download_file", { remotePath, localPath, transferId }),

  getDownloadsDir: () =>
    invoke<string>("cmd_get_downloads_dir"),

  createFolder: (path: string) =>
    invoke<void>("cmd_create_folder", { path }),

  renameFile: (oldPath: string, newPath: string) =>
    invoke<void>("cmd_rename_file", { oldPath, newPath }),

  deleteFile: (path: string) =>
    invoke<void>("cmd_delete_file", { path }),
}
