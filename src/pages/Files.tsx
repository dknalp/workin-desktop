import React, { useCallback, useEffect, useRef, useState } from "react"
import { open } from "@tauri-apps/plugin-dialog"
import {
  FileIcon, FolderIcon, HomeIcon, ChevronRightIcon,
  UploadIcon, RefreshCwIcon, AlertCircleIcon, FolderPlusIcon,
} from "lucide-react"
import { Button } from "../components/ui/button"
import { ContextMenu } from "../components/ui/context-menu"
import { commands } from "../lib/commands"
import { useTransferStore } from "../store"
import { cn, formatBytes } from "../lib/utils"

interface FileEntry {
  name: string
  path: string
  is_dir: boolean
  size?: number
  updated_at?: string
}

interface CtxState {
  x: number
  y: number
  entry: FileEntry | null
}

export function Files() {
  const [currentPath, setCurrentPath] = useState("/")
  const [entries, setEntries] = useState<FileEntry[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [dragging, setDragging] = useState(false)
  const [ctx, setCtx] = useState<CtxState | null>(null)
  const [renaming, setRenaming] = useState<{ path: string; name: string } | null>(null)
  const [newFolderName, setNewFolderName] = useState<string | null>(null)
  const [confirmDelete, setConfirmDelete] = useState<FileEntry | null>(null)
  const addTransfer = useTransferStore((s) => s.addTransfer)
  const dropRef = useRef<HTMLDivElement>(null)

  const breadcrumbs = currentPath === "/" ? [] : currentPath.split("/").filter(Boolean)

  const load = useCallback(async (path: string) => {
    setLoading(true); setError(null)
    try {
      const result = await commands.listFiles(path)
      const data = result as { items?: FileEntry[] } | FileEntry[]
      setEntries(Array.isArray(data) ? data : ((data as { items?: FileEntry[] }).items ?? []))
    } catch (err: unknown) {
      setError((err as { message?: string })?.message ?? "Failed to load files")
    } finally { setLoading(false) }
  }, [])

  useEffect(() => { load(currentPath) }, [currentPath, load])

  const startUpload = useCallback(async (localPaths: string[]) => {
    for (const localPath of localPaths) {
      const fileName = localPath.split(/[/\\]/).pop() ?? localPath
      const remotePath = currentPath === "/" ? `/${fileName}` : `${currentPath}/${fileName}`
      const id = `${Date.now()}-${Math.random().toString(36).slice(2)}`
      addTransfer({ id, kind: "upload", fileName, remotePath, localPath, bytesDone: 0, totalBytes: 0, speedBps: 0, status: "queued", startedAt: Date.now() })
      commands.uploadFile(localPath, remotePath, id).then(() => load(currentPath)).catch(() => {})
    }
  }, [currentPath, addTransfer, load])

  const startDownload = useCallback(async (entry: FileEntry) => {
    const downloadsDir = await commands.getDownloadsDir().catch(() => "")
    const localPath = downloadsDir ? `${downloadsDir}/${entry.name}` : entry.name
    const id = `${Date.now()}-${Math.random().toString(36).slice(2)}`
    addTransfer({ id, kind: "download", fileName: entry.name, remotePath: entry.path, localPath, bytesDone: 0, totalBytes: entry.size ?? 0, speedBps: 0, status: "queued", startedAt: Date.now() })
    commands.downloadFile(entry.path, localPath, id).catch(() => {})
  }, [addTransfer])

  const handlePickFiles = async () => {
    const selected = await open({ multiple: true, directory: false })
    if (!selected) return
    await startUpload(Array.isArray(selected) ? selected : [selected])
  }

  const handleCreateFolder = async (name: string) => {
    const path = currentPath === "/" ? `/${name}` : `${currentPath}/${name}`
    await commands.createFolder(path).catch(() => {})
    setNewFolderName(null)
    load(currentPath)
  }

  const handleRename = async (oldPath: string, newName: string) => {
    const parts = oldPath.split("/")
    parts[parts.length - 1] = newName
    const newPath = parts.join("/")
    await commands.renameFile(oldPath, newPath).catch(() => {})
    setRenaming(null)
    load(currentPath)
  }

  const handleDelete = async (entry: FileEntry) => {
    await commands.deleteFile(entry.path).catch(() => {})
    setConfirmDelete(null)
    load(currentPath)
  }

  // Drag-and-drop
  useEffect(() => {
    const el = dropRef.current; if (!el) return
    const onDragOver = (e: DragEvent) => { e.preventDefault(); setDragging(true) }
    const onDragLeave = () => setDragging(false)
    const onDrop = async (e: DragEvent) => {
      e.preventDefault(); setDragging(false)
      const files = Array.from(e.dataTransfer?.files ?? [])
      const paths = files.map((f) => (f as unknown as { path: string }).path).filter(Boolean)
      if (paths.length > 0) await startUpload(paths)
    }
    el.addEventListener("dragover", onDragOver); el.addEventListener("dragleave", onDragLeave); el.addEventListener("drop", onDrop)
    return () => { el.removeEventListener("dragover", onDragOver); el.removeEventListener("dragleave", onDragLeave); el.removeEventListener("drop", onDrop) }
  }, [startUpload])

  const openCtx = (e: React.MouseEvent, entry: FileEntry | null) => {
    e.preventDefault(); setCtx({ x: e.clientX, y: e.clientY, entry })
  }

  const ctxItems = ctx?.entry
    ? [
        { label: "Download", onClick: () => ctx.entry && startDownload(ctx.entry), disabled: ctx.entry.is_dir },
        "separator" as const,
        { label: "Rename", onClick: () => ctx.entry && setRenaming({ path: ctx.entry.path, name: ctx.entry.name }) },
        { label: "Delete", onClick: () => ctx.entry && setConfirmDelete(ctx.entry), danger: true },
      ]
    : [
        { label: "Upload files", onClick: handlePickFiles },
        { label: "New folder", onClick: () => setNewFolderName("New Folder") },
      ]

  return (
    <div ref={dropRef} className={cn("flex flex-col h-full relative", dragging && "ring-2 ring-inset ring-blue-500")}>
      {/* Toolbar */}
      <div className="flex items-center gap-2 px-4 py-2 border-b border-border bg-card">
        <button onClick={() => setCurrentPath("/")} className="p-1 rounded hover:bg-muted text-muted-foreground hover:text-foreground">
          <HomeIcon className="h-4 w-4" />
        </button>
        {breadcrumbs.map((seg, i) => {
          const path = "/" + breadcrumbs.slice(0, i + 1).join("/")
          return (
            <React.Fragment key={path}>
              <ChevronRightIcon className="h-3.5 w-3.5 text-muted-foreground" />
              <button onClick={() => setCurrentPath(path)} className="text-sm hover:text-foreground text-muted-foreground transition-colors">{seg}</button>
            </React.Fragment>
          )
        })}
        <div className="ml-auto flex items-center gap-2">
          <Button variant="ghost" size="sm" onClick={() => setNewFolderName("New Folder")}>
            <FolderPlusIcon className="h-3.5 w-3.5" />
          </Button>
          <Button variant="ghost" size="sm" onClick={() => load(currentPath)} disabled={loading}>
            <RefreshCwIcon className={cn("h-3.5 w-3.5", loading && "animate-spin")} />
          </Button>
          <Button size="sm" onClick={handlePickFiles} className="gap-1.5">
            <UploadIcon className="h-3.5 w-3.5" /> Upload
          </Button>
        </div>
      </div>

      {/* File list */}
      <div className="flex-1 overflow-auto" onContextMenu={(e) => { if (e.target === e.currentTarget) openCtx(e, null) }}>
        {error ? (
          <div className="flex flex-col items-center justify-center h-full gap-3 text-muted-foreground">
            <AlertCircleIcon className="h-8 w-8 text-red-500" />
            <p className="text-sm">{error}</p>
            <Button variant="outline" size="sm" onClick={() => load(currentPath)}>Retry</Button>
          </div>
        ) : loading && entries.length === 0 ? (
          <div className="flex items-center justify-center h-full">
            <span className="h-5 w-5 rounded-full border-2 border-blue-600 border-t-transparent animate-spin" />
          </div>
        ) : entries.length === 0 ? (
          <div className="flex flex-col items-center justify-center h-full gap-2 text-muted-foreground" onContextMenu={(e) => openCtx(e, null)}>
            <FolderIcon className="h-10 w-10 opacity-30" />
            <p className="text-sm">No files here</p>
            <p className="text-xs">Drag files here or click Upload</p>
          </div>
        ) : (
          <table className="w-full text-sm">
            <thead className="sticky top-0 bg-background border-b border-border">
              <tr>
                <th className="text-left px-4 py-2 font-medium text-muted-foreground">Name</th>
                <th className="text-right px-4 py-2 font-medium text-muted-foreground w-28">Size</th>
                <th className="text-right px-4 py-2 font-medium text-muted-foreground w-40">Modified</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-border">
              {newFolderName !== null && (
                <tr className="bg-accent/30">
                  <td className="px-4 py-2.5" colSpan={3}>
                    <div className="flex items-center gap-2.5">
                      <FolderIcon className="h-4 w-4 text-blue-500 shrink-0" />
                      <input
                        autoFocus
                        className="text-sm bg-transparent border-b border-blue-600 outline-none w-48"
                        value={newFolderName}
                        onChange={(e) => setNewFolderName(e.target.value)}
                        onKeyDown={(e) => {
                          if (e.key === "Enter") handleCreateFolder(newFolderName)
                          if (e.key === "Escape") setNewFolderName(null)
                        }}
                        onBlur={() => { if (newFolderName.trim()) handleCreateFolder(newFolderName); else setNewFolderName(null) }}
                      />
                    </div>
                  </td>
                </tr>
              )}
              {entries.map((entry) => (
                <tr
                  key={entry.path}
                  className="hover:bg-muted/50 cursor-pointer"
                  onDoubleClick={() => entry.is_dir && setCurrentPath(entry.path)}
                  onContextMenu={(e) => openCtx(e, entry)}
                >
                  <td className="px-4 py-2.5">
                    <div className="flex items-center gap-2.5">
                      {entry.is_dir ? <FolderIcon className="h-4 w-4 text-blue-500 shrink-0" /> : <FileIcon className="h-4 w-4 text-muted-foreground shrink-0" />}
                      {renaming?.path === entry.path ? (
                        <input
                          autoFocus
                          className="text-sm bg-transparent border-b border-blue-600 outline-none"
                          value={renaming.name}
                          onChange={(e) => setRenaming({ ...renaming, name: e.target.value })}
                          onKeyDown={(e) => {
                            if (e.key === "Enter") handleRename(entry.path, renaming.name)
                            if (e.key === "Escape") setRenaming(null)
                          }}
                          onBlur={() => handleRename(entry.path, renaming.name)}
                        />
                      ) : (
                        <span className="truncate">{entry.name}</span>
                      )}
                    </div>
                  </td>
                  <td className="px-4 py-2.5 text-right text-muted-foreground tabular-nums">{entry.is_dir ? "—" : formatBytes(entry.size ?? 0)}</td>
                  <td className="px-4 py-2.5 text-right text-muted-foreground">{entry.updated_at ? new Date(entry.updated_at).toLocaleDateString() : "—"}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      {/* Context menu */}
      {ctx && <ContextMenu x={ctx.x} y={ctx.y} onClose={() => setCtx(null)} items={ctxItems} />}

      {/* Delete confirm dialog */}
      {confirmDelete && (
        <div className="absolute inset-0 flex items-center justify-center bg-black/30 z-40">
          <div className="bg-card border border-border rounded-lg p-5 w-80 shadow-xl space-y-3">
            <h3 className="font-medium text-sm">Delete "{confirmDelete.name}"?</h3>
            <p className="text-xs text-muted-foreground">This action cannot be undone.</p>
            <div className="flex gap-2 justify-end">
              <Button variant="outline" size="sm" onClick={() => setConfirmDelete(null)}>Cancel</Button>
              <Button variant="destructive" size="sm" onClick={() => handleDelete(confirmDelete)}>Delete</Button>
            </div>
          </div>
        </div>
      )}

      {dragging && (
        <div className="absolute inset-0 flex items-center justify-center bg-blue-500/10 pointer-events-none">
          <div className="flex flex-col items-center gap-2 text-blue-600">
            <UploadIcon className="h-10 w-10" />
            <p className="text-sm font-medium">Drop to upload</p>
          </div>
        </div>
      )}
    </div>
  )
}
