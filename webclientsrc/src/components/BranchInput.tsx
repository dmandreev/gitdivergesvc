import { useState, useEffect, useRef, useMemo, useCallback } from 'react'
import { GitBranch, Loader2, AlertCircle, RotateCcw } from 'lucide-react'
import { listBranches } from '../api/client'

interface BranchInputProps {
  value: string
  onChange: (value: string) => void
  onSubmit: () => void
  repoGuid: string
  token: string | null
  disabled?: boolean
}

export function BranchInput({
  value,
  onChange,
  onSubmit,
  repoGuid,
  token,
  disabled = false,
}: BranchInputProps) {
  const [allBranches, setAllBranches] = useState<string[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [open, setOpen] = useState(false)
  const [highlightedIndex, setHighlightedIndex] = useState(-1)
  const containerRef = useRef<HTMLDivElement>(null)
  const itemRefs = useRef<(HTMLLIElement | null)[]>([])

  const loadBranches = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const { data, error: apiError } = await listBranches({
        path: { repo_guid: repoGuid },
        headers: token ? { Authorization: `Bearer ${token}` } : undefined,
      })
      if (apiError) throw apiError
      const branches = ((data as { branches?: string[] } | undefined)?.branches ?? [])
        .slice()
        .sort((a, b) => a.localeCompare(b))
      setAllBranches(branches)
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to load branches')
    } finally {
      setLoading(false)
    }
  }, [repoGuid, token])

  useEffect(() => {
    if (disabled) return
    setAllBranches([])
    setError(null)
    setOpen(false)
    loadBranches()
  }, [disabled, loadBranches])

  useEffect(() => {
    function handleMouseDown(e: MouseEvent) {
      if (
        containerRef.current &&
        !containerRef.current.contains(e.target as Node)
      ) {
        setOpen(false)
      }
    }
    document.addEventListener('mousedown', handleMouseDown)
    return () => document.removeEventListener('mousedown', handleMouseDown)
  }, [])

  const segments = value.split(',')
  const currentSegment = segments[segments.length - 1]
  const previousBranches = segments
    .slice(0, -1)
    .map((s) => s.trim())
    .filter(Boolean)
  const query = currentSegment.trim()

  const suggestions = useMemo(() => {
    const selected = new Set(previousBranches)
    const q = query.toLowerCase()
    return allBranches
      .filter((b) => !selected.has(b))
      .filter((b) => b.toLowerCase().includes(q))
  }, [allBranches, previousBranches, query])

  useEffect(() => {
    setHighlightedIndex(-1)
  }, [suggestions.length, query])

  useEffect(() => {
    const el = itemRefs.current[highlightedIndex]
    if (el) {
      el.scrollIntoView({ block: 'nearest' })
    }
  }, [highlightedIndex])

  const handleInputChange = (newValue: string) => {
    onChange(newValue)
    if (!open) setOpen(true)
  }

  const handleSelect = (branch: string) => {
    const newSegments = [...segments]
    newSegments[newSegments.length - 1] = branch
    onChange(newSegments.join(', ') + ', ')
    setHighlightedIndex(-1)
  }

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      if (!open) {
        setOpen(true)
      } else if (suggestions.length > 0) {
        setHighlightedIndex((i) => (i + 1) % suggestions.length)
      }
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      if (!open) {
        setOpen(true)
      } else if (suggestions.length > 0) {
        setHighlightedIndex(
          (i) => (i - 1 + suggestions.length) % suggestions.length
        )
      }
    } else if (e.key === 'Enter') {
      if (
        open &&
        highlightedIndex >= 0 &&
        highlightedIndex < suggestions.length
      ) {
        e.preventDefault()
        handleSelect(suggestions[highlightedIndex])
      } else {
        setOpen(false)
        onSubmit()
      }
    } else if (e.key === 'Escape') {
      setOpen(false)
    } else if (e.key === 'Tab') {
      setOpen(false)
    }
  }

  const showDropdown = open && !disabled
  const listboxId = 'branch-suggestions'
  const getOptionId = (index: number) => `branch-option-${index}`

  return (
    <div ref={containerRef} className="relative">
      <input
        type="text"
        value={value}
        onChange={(e) => handleInputChange(e.target.value)}
        onFocus={() => setOpen(true)}
        onKeyDown={handleKeyDown}
        placeholder="main, develop, stage"
        disabled={disabled}
        className="w-full px-3 py-2 rounded-lg bg-surface-1 border border-surface-3 text-sm text-text-main placeholder:text-text-dim focus:outline-none focus:border-accent/50 focus:ring-1 focus:ring-accent/30 transition-all disabled:opacity-50"
        aria-autocomplete="list"
        aria-expanded={showDropdown}
        aria-controls={showDropdown ? listboxId : undefined}
        aria-activedescendant={
          showDropdown && highlightedIndex >= 0
            ? getOptionId(highlightedIndex)
            : undefined
        }
      />

      {showDropdown && (
        <div
          id={listboxId}
          role="listbox"
          className="absolute z-50 w-full mt-1.5 rounded-xl bg-surface-2 border border-surface-3 shadow-2xl shadow-black/50 overflow-hidden"
        >
          {loading ? (
            <div className="flex items-center gap-2 px-3 py-3 text-sm text-text-muted">
              <Loader2 className="w-3.5 h-3.5 animate-spin" />
              Loading branches…
            </div>
          ) : error ? (
            <div className="px-3 py-3 space-y-2">
              <div className="flex items-center gap-2 text-sm text-red">
                <AlertCircle className="w-3.5 h-3.5" />
                Failed to load branches
              </div>
              <button
                onClick={loadBranches}
                className="flex items-center gap-1.5 text-xs text-text-muted hover:text-text-main transition-colors"
              >
                <RotateCcw className="w-3 h-3" />
                Retry
              </button>
            </div>
          ) : allBranches.length === 0 ? (
            <div className="px-3 py-3 text-sm text-text-muted">
              No branches found
            </div>
          ) : suggestions.length === 0 ? (
            <div className="px-3 py-3 text-sm text-text-muted">
              No matching branches
            </div>
          ) : (
            <ul className="max-h-64 overflow-y-auto py-1">
              {suggestions.map((branch, index) => {
                const isHighlighted = index === highlightedIndex
                return (
                  <li
                    key={branch}
                    id={getOptionId(index)}
                    ref={(el) => {
                      itemRefs.current[index] = el
                    }}
                    role="option"
                    aria-selected={isHighlighted}
                    onMouseEnter={() => setHighlightedIndex(index)}
                    onMouseDown={(e) => {
                      e.preventDefault()
                      handleSelect(branch)
                    }}
                    className={`flex items-center gap-2 px-3 py-2 text-sm cursor-pointer transition-colors ${
                      isHighlighted
                        ? 'bg-accent/10 text-text-main'
                        : 'text-text-muted hover:bg-surface-3 hover:text-text-main'
                    }`}
                  >
                    <GitBranch
                      className={`w-3.5 h-3.5 shrink-0 ${
                        isHighlighted
                          ? 'text-accent-light'
                          : 'text-text-dim'
                      }`}
                    />
                    <span className="truncate">
                      <HighlightMatch text={branch} query={query} />
                    </span>
                  </li>
                )
              })}
            </ul>
          )}
        </div>
      )}
    </div>
  )
}

function HighlightMatch({
  text,
  query,
}: {
  text: string
  query: string
}) {
  if (!query) return <>{text}</>
  const lowerText = text.toLowerCase()
  const lowerQuery = query.toLowerCase()
  const idx = lowerText.indexOf(lowerQuery)
  if (idx === -1) return <>{text}</>
  return (
    <>
      {text.slice(0, idx)}
      <span className="text-accent-light font-semibold">
        {text.slice(idx, idx + query.length)}
      </span>
      {text.slice(idx + query.length)}
    </>
  )
}
