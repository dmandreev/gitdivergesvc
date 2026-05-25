import { useRef, useState, useEffect, useCallback, useMemo } from 'react'

interface VirtualListProps<T> {
  items: T[]
  itemHeight: number
  overscan?: number
  renderItem: (item: T, index: number) => React.ReactNode
  className?: string
}

export function VirtualList<T>({
  items,
  itemHeight,
  overscan = 8,
  renderItem,
  className,
}: VirtualListProps<T>) {
  const containerRef = useRef<HTMLDivElement>(null)
  const [scrollTop, setScrollTop] = useState(0)
  const [height, setHeight] = useState(0)

  useEffect(() => {
    const el = containerRef.current
    if (!el) return

    const update = () => setHeight(el.clientHeight)
    update()

    const ro = new ResizeObserver(update)
    ro.observe(el)
    return () => ro.disconnect()
  }, [])

  const { totalHeight, startIndex, endIndex, offsetY } = useMemo(() => {
    const total = items.length * itemHeight
    if (height === 0) {
      // Fallback for SSR / test environments without layout metrics
      return { totalHeight: total, startIndex: 0, endIndex: items.length, offsetY: 0 }
    }
    const start = Math.max(0, Math.floor(scrollTop / itemHeight) - overscan)
    const end = Math.min(
      items.length,
      Math.ceil((scrollTop + height) / itemHeight) + overscan
    )
    const offset = start * itemHeight
    return { totalHeight: total, startIndex: start, endIndex: end, offsetY: offset }
  }, [items.length, itemHeight, scrollTop, height, overscan])

  const handleScroll = useCallback((e: React.UIEvent<HTMLDivElement>) => {
    setScrollTop(e.currentTarget.scrollTop)
  }, [])

  return (
    <div
      ref={containerRef}
      onScroll={handleScroll}
      className={`overflow-auto ${className ?? ''}`}
    >
      <div style={{ height: totalHeight, position: 'relative' }}>
        <div style={{ transform: `translateY(${offsetY}px)` }}>
          {items.slice(startIndex, endIndex).map((item, i) => (
            <div
              key={startIndex + i}
              style={{ height: itemHeight }}
              className="overflow-hidden"
            >
              {renderItem(item, startIndex + i)}
            </div>
          ))}
        </div>
      </div>
    </div>
  )
}
