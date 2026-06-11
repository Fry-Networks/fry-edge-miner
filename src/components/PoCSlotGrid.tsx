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

  // Count passing slots
  const allPassCount = slots.filter(
    (s) => s.data && s.online && s.mac_match && s.pol && s.poi && s.poa
  ).length

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
    <div className="space-y-3">
      {/* Header with count + legend */}
      <div className="flex items-center justify-between">
        <span className="text-xs text-fry-text-muted">
          {allPassCount}/{slots.length} slots passing all gates
        </span>
        <div className="flex gap-3 text-xs text-fry-text-muted">
          <div className="flex items-center gap-1.5">
            <div className="w-2.5 h-2.5 rounded-sm bg-fry-neon" />
            <span>Pass</span>
          </div>
          <div className="flex items-center gap-1.5">
            <div className="w-2.5 h-2.5 rounded-sm bg-fry-warning" />
            <span>Partial</span>
          </div>
          <div className="flex items-center gap-1.5">
            <div className="w-2.5 h-2.5 rounded-sm bg-fry-error" />
            <span>Fail</span>
          </div>
          <div className="flex items-center gap-1.5">
            <div className="w-2.5 h-2.5 rounded-sm bg-fry-border" />
            <span>Empty</span>
          </div>
        </div>
      </div>

      {/* Grid */}
      <div className="grid grid-cols-12 gap-[3px] bg-fry-border/30 p-4 rounded-xl">
        {cells.map((cell) => (
          <div
            key={cell.index}
            className="relative group"
            onMouseEnter={() => setHoveredSlot(cell.index)}
            onMouseLeave={() => setHoveredSlot(null)}
          >
            <div
              className={`w-full aspect-square rounded-sm ${cell.color} cursor-default transition opacity-80 hover:opacity-100`}
              title={
                cell.slot
                  ? `Slot ${cell.index}: ${cell.slot.tools_count} tools`
                  : 'No data'
              }
            />

            {/* Tooltip — flips below for top 2 rows */}
            {hoveredSlot === cell.index && cell.slot && (
              <div
                className={`absolute left-1/2 transform -translate-x-1/2 bg-fry-bg border border-fry-border rounded px-3 py-2 text-xs text-fry-text whitespace-nowrap z-50 ${
                  cell.index < 24
                    ? 'top-full mt-1'
                    : 'bottom-full mb-2'
                }`}
              >
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
    </div>
  )
}
