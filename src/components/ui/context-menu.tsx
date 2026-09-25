import { useEffect, useRef } from "react"
import { cn } from "../../lib/utils"

interface ContextMenuProps {
  x: number
  y: number
  onClose: () => void
  items: Array<{ label: string; onClick: () => void; danger?: boolean; disabled?: boolean } | "separator">
}

export function ContextMenu({ x, y, onClose, items }: ContextMenuProps) {
  const ref = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const handle = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose()
    }
    document.addEventListener("mousedown", handle)
    return () => document.removeEventListener("mousedown", handle)
  }, [onClose])

  return (
    <div
      ref={ref}
      style={{ left: x, top: y }}
      className="fixed z-50 min-w-40 rounded-md border border-border bg-card shadow-lg py-1"
    >
      {items.map((item, i) =>
        item === "separator" ? (
          <div key={i} className="my-1 border-t border-border" />
        ) : (
          <button
            key={i}
            disabled={item.disabled}
            onClick={() => { item.onClick(); onClose() }}
            className={cn(
              "w-full text-left px-3 py-1.5 text-sm transition-colors",
              item.danger ? "text-red-500 hover:bg-red-500/10" : "hover:bg-muted",
              item.disabled && "opacity-40 pointer-events-none"
            )}
          >
            {item.label}
          </button>
        )
      )}
    </div>
  )
}
