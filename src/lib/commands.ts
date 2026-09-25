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
}
