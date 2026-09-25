import { useEffect } from "react"
import { listen } from "@tauri-apps/api/event"
import { useTransferStore } from "../store"

interface UploadProgressEvent {
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
    let unlisten: (() => void) | undefined

    listen<UploadProgressEvent>("upload_progress", (event) => {
      const { transfer_id, bytes_done, total_bytes, speed_bps, status, error } = event.payload
      updateTransfer(transfer_id, {
        bytesDone: bytes_done,
        totalBytes: total_bytes,
        speedBps: speed_bps,
        status: status as "uploading" | "complete" | "error",
        error,
      })
    }).then((fn) => { unlisten = fn })

    return () => { unlisten?.() }
  }, [updateTransfer])
}
