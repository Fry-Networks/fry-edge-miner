import { useState } from 'react'
import type { PocSlot } from '../lib/types'

interface PoCSlotGridProps {
  slots: PocSlot[]
}

export function PoCSlotGrid({ slots }: PoCSlotGridProps) {
  const [hoveredSlot, setHoveredSlot] = useState<number | null>(null)
  const GRID_SIZE = 144 // 12x12

  // Create a map of slot_index -> slot for fast lookup
  const slotMap = new Map(slots.map((s) => [s.slot_index, s]))

  // Generate 144 cells
  const cells = Array.from({ length: GRID_SIZE }).map((_, idx) => {
    const slot = slotMap.get(idx)
    if (!slot) {
      return { index: idx, color: 'bg-fry-border', slot: null }
    }

    // Determine color based on gates
    const allGatesPassing =
      slot.data &&
      slot.online &&
      slot.mac_match &&
      slot.pol &&
      slot.poi &&
      slot.poa

    if (allGatesPassing) {
      return { index: idx, color: 'bg-fry-neon', slot }
    } else if (
      slot.data ||
      slot.online ||
      slot.mac_match ||
      slot.pol ||
      slot.poi ||
      slot.poa
    ) {
      return { index: idx, color: 'bg-fry-warning', slot }
    } else {
      return { index: idx, color: 'bg-fry-error', slot }
    }
  })

  return (
    <div className="space-y-4">
      {/* Grid */}
      <div className="grid grid-cols-12 gap-1 bg-fry-border/30 p-4 rounded-xl">
        {cells.map((cell) => (
          <div
            key={cell.index}
            className="relative group"
            onMouseEnter={() => setHoveredSlot(cell.index)}
            onMouseLeave={() => setHoveredSlot(null)}
          >
            <div
              className={`w-full aspect-square rounded ${cell.color} cursor-default transition opacity-80 hover:opacity-100`}
              title={
                cell.slot
                  ? `Slot ${cell.index}: ${cell.slot.tools_count} tools`
                  : 'No data'
              }
            />

            {/* Tooltip */}
            {hoveredSlot === cell.index && cell.slot && (
              <div className="absolute bottom-full left-1/2 transform -translate-x-1/2 mb-2 bg-fry-bg border border-fry-border rounded px-3 py-2 text-xs text-fry-text whitespace-nowrap z-10">
                <div className="font-semibold">Slot {cell.index}</div>
                <div className="text-fry-text-muted">
                  {cell.slot.tools_count} tools
                </div>
                <div className="text-fry-text-muted">
                  Multiplier: {cell.slot.multiplier.toFixed(2)}x
                </div>
              </div>
            )}
          </div>
        ))}
      </div>

      {/* Legend */}
      <div className="flex gap-4 text-xs text-fry-text-muted">
        <div className="flex items-center gap-2">
          <div className="w-3 h-3 rounded bg-fry-neon" />
          <span>All gates passing</span>
        </div>
        <div className="flex items-center gap-2">
          <div className="w-3 h-3 rounded bg-fry-warning" />
          <span>Partial</span>
        </div>
        <div className="flex items-center gap-2">
          <div className="w-3 h-3 rounded bg-fry-error" />
          <span>Failed</span>
        </div>
        <div className="flex items-center gap-2">
          <div className="w-3 h-3 rounded bg-fry-border" />
          <span>No data</span>
        </div>
      </div>
    </div>
  )
}
