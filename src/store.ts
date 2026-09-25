import { create } from "zustand"
import type { SessionInfo } from "./lib/commands"

export type TransferStatus = "queued" | "uploading" | "downloading" | "complete" | "error"
export type TransferKind = "upload" | "download"

export interface Transfer {
  id: string
  kind: TransferKind
  fileName: string
  remotePath: string
  localPath: string
  bytesDone: number
  totalBytes: number
  speedBps: number
  status: TransferStatus
  error?: string
  startedAt: number
}

interface AuthState {
  session: SessionInfo | null
  setSession: (s: SessionInfo | null) => void
}

interface TransferState {
  transfers: Transfer[]
  addTransfer: (t: Transfer) => void
  updateTransfer: (id: string, patch: Partial<Transfer>) => void
  clearCompleted: () => void
}

export const useAuthStore = create<AuthState>((set) => ({
  session: null,
  setSession: (session) => set({ session }),
}))

export const useTransferStore = create<TransferState>((set) => ({
  transfers: [],
  addTransfer: (t) => set((s) => ({ transfers: [...s.transfers, t] })),
  updateTransfer: (id, patch) =>
    set((s) => ({
      transfers: s.transfers.map((t) => (t.id === id ? { ...t, ...patch } : t)),
    })),
  clearCompleted: () =>
    set((s) => ({ transfers: s.transfers.filter((t) => t.status !== "complete") })),
}))
