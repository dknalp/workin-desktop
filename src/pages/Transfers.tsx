import { CheckCircleIcon, XCircleIcon, UploadIcon, DownloadIcon } from "lucide-react"
import { Progress } from "../components/ui/progress"
import { Button } from "../components/ui/button"
import { useTransferStore, type Transfer } from "../store"
import { cn, formatBytes, formatSpeed, formatEta } from "../lib/utils"

function TransferItem({ t }: { t: Transfer }) {
  const pct = t.totalBytes > 0 ? (t.bytesDone / t.totalBytes) * 100 : 0
  const active = t.status === "uploading" || t.status === "downloading"
  const KindIcon = t.kind === "download" ? DownloadIcon : UploadIcon

  return (
    <div className="px-4 py-3 space-y-1.5">
      <div className="flex items-center gap-2">
        {t.status === "complete" && <CheckCircleIcon className="h-4 w-4 shrink-0 text-green-500" />}
        {t.status === "error"    && <XCircleIcon    className="h-4 w-4 shrink-0 text-red-500" />}
        {(active || t.status === "queued") && <KindIcon className="h-4 w-4 shrink-0 text-blue-500" />}
        <span className="text-sm font-medium truncate flex-1">{t.fileName}</span>
        <span className={cn("text-xs shrink-0",
          t.status === "complete" && "text-green-500",
          t.status === "error"    && "text-red-500",
          (active || t.status === "queued") && "text-muted-foreground",
        )}>
          {t.status === "complete"   && "Done"}
          {t.status === "error"      && "Failed"}
          {t.status === "queued"     && "Queued"}
          {active && `${formatBytes(t.bytesDone)} / ${formatBytes(t.totalBytes)}`}
        </span>
      </div>
      {active && (
        <>
          <Progress value={pct} />
          <div className="flex justify-between text-xs text-muted-foreground">
            <span>{formatSpeed(t.speedBps)}</span>
            <span>ETA {formatEta(t.totalBytes - t.bytesDone, t.speedBps)}</span>
          </div>
        </>
      )}
      {t.status === "error" && t.error && <p className="text-xs text-red-500">{t.error}</p>}
    </div>
  )
}

export function Transfers() {
  const transfers      = useTransferStore((s) => s.transfers)
  const clearCompleted = useTransferStore((s) => s.clearCompleted)
  const hasCompleted   = transfers.some((t) => t.status === "complete")

  return (
    <div className="flex flex-col h-full">
      <div className="flex items-center justify-between px-4 py-2 border-b border-border bg-card">
        <h2 className="text-sm font-medium">Transfers</h2>
        {hasCompleted && <Button variant="ghost" size="sm" onClick={clearCompleted}>Clear completed</Button>}
      </div>
      <div className="flex-1 overflow-auto divide-y divide-border">
        {transfers.length === 0 ? (
          <div className="flex flex-col items-center justify-center h-full gap-2 text-muted-foreground">
            <UploadIcon className="h-8 w-8 opacity-30" />
            <p className="text-sm">No transfers</p>
          </div>
        ) : (
          [...transfers].reverse().map((t) => <TransferItem key={t.id} t={t} />)
        )}
      </div>
    </div>
  )
}
