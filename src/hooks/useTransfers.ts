import { useEffect } from "react"
import { listen } from "@tauri-apps/api/event"
import { useTransferStore } from "../store"

interface ProgressEvent {
  transfer_id: string
  path: string
  bytes_done: number
  total_bytes: number
  speed_bps: number
  status: string
  error?: string
}

export function useTransferEvents() {
  const updateTransfer = useTransferStore((s) => s.updateTransfer)

  useEffect(() => {
    const unlisteners: Array<() => void> = []

    const handler = (event: { payload: ProgressEvent }) => {
      const { transfer_id, bytes_done, total_bytes, speed_bps, status, error } = event.payload
      updateTransfer(transfer_id, {
        bytesDone: bytes_done,
        totalBytes: total_bytes,
        speedBps: speed_bps,
        status: status as "uploading" | "downloading" | "complete" | "error",
        error,
      })
    }

    listen<ProgressEvent>("upload_progress", handler).then((fn) => unlisteners.push(fn))
    listen<ProgressEvent>("download_progress", handler).then((fn) => unlisteners.push(fn))

    return () => unlisteners.forEach((fn) => fn())
  }, [updateTransfer])
}
