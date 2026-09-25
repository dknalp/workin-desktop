import { useEffect, useState } from "react"
import { FolderIcon, ArrowUpIcon, LogOutIcon } from "lucide-react"
import { Setup } from "./pages/Setup"
import { Files } from "./pages/Files"
import { Transfers } from "./pages/Transfers"
import { commands } from "./lib/commands"
import { useAuthStore, useTransferStore } from "./store"
import { useTransferEvents } from "./hooks/useTransfers"
import { cn } from "./lib/utils"

type Tab = "files" | "transfers"

function Shell() {
  const [tab, setTab] = useState<Tab>("files")
  const session = useAuthStore((s) => s.session)
  const setSession = useAuthStore((s) => s.setSession)
  const activeTransfers = useTransferStore((s) =>
    s.transfers.filter((t) => t.status === "uploading" || t.status === "queued").length
  )
  useTransferEvents()

  const logout = async () => {
    await commands.logout()
    setSession(null)
  }

  return (
    <div className="flex h-screen bg-background text-foreground">
      <aside className="w-48 shrink-0 flex flex-col border-r border-border bg-card">
        <div className="px-4 py-3 border-b border-border">
          <span className="text-sm font-bold">workin</span>
          <p className="text-xs text-muted-foreground truncate mt-0.5">{session?.user_email}</p>
        </div>
        <nav className="flex-1 p-2 space-y-0.5">
          <button
            onClick={() => setTab("files")}
            className={cn("w-full flex items-center gap-2.5 px-3 py-2 rounded-md text-sm transition-colors",
              tab === "files" ? "bg-accent text-accent-foreground" : "text-muted-foreground hover:bg-muted hover:text-foreground")}
          >
            <FolderIcon className="h-4 w-4" />Files
          </button>
          <button
            onClick={() => setTab("transfers")}
            className={cn("w-full flex items-center gap-2.5 px-3 py-2 rounded-md text-sm transition-colors",
              tab === "transfers" ? "bg-accent text-accent-foreground" : "text-muted-foreground hover:bg-muted hover:text-foreground")}
          >
            <ArrowUpIcon className="h-4 w-4" />Transfers
            {activeTransfers > 0 && (
              <span className="ml-auto text-xs bg-blue-600 text-white rounded-full px-1.5 py-0.5 leading-none">{activeTransfers}</span>
            )}
          </button>
        </nav>
        <div className="p-2 border-t border-border">
          <button onClick={logout} className="w-full flex items-center gap-2.5 px-3 py-2 rounded-md text-sm text-muted-foreground hover:bg-muted hover:text-foreground transition-colors">
            <LogOutIcon className="h-4 w-4" />Sign out
          </button>
        </div>
      </aside>
      <main className="flex-1 min-w-0 relative">
        {tab === "files" && <Files />}
        {tab === "transfers" && <Transfers />}
      </main>
    </div>
  )
}

export default function App() {
  const session = useAuthStore((s) => s.session)
  const setSession = useAuthStore((s) => s.setSession)
  const [checking, setChecking] = useState(true)

  useEffect(() => {
    commands.restoreSession()
      .then((s) => { if (s) setSession(s) })
      .finally(() => setChecking(false))
  }, [setSession])

  if (checking) {
    return (
      <div className="flex h-screen items-center justify-center bg-background">
        <span className="h-6 w-6 rounded-full border-2 border-blue-600 border-t-transparent animate-spin" />
      </div>
    )
  }

  return session ? <Shell /> : <Setup />
}
