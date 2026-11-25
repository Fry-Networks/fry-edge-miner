// Pure TypeScript ISO/IEC 18004 QR Code encoder (Byte mode, EC level L, versions 1..10).
// Zero external dependencies.

interface VersionSpec {
  version: number
  size: number
  dataCodewords: number
  ecCodewordsPerBlock: number
  group1Blocks: number
  group1Data: number
  group2Blocks: number
  group2Data: number
  alignment: number[]
}

// ISO/IEC 18004 Table 7 & Table 9 for EC Level L (versions 1..10) and Table E.1 alignment centers.
const VERSIONS_L: VersionSpec[] = [
  { version: 1, size: 21, dataCodewords: 19, ecCodewordsPerBlock: 7, group1Blocks: 1, group1Data: 19, group2Blocks: 0, group2Data: 0, alignment: [] },
  { version: 2, size: 25, dataCodewords: 34, ecCodewordsPerBlock: 10, group1Blocks: 1, group1Data: 34, group2Blocks: 0, group2Data: 0, alignment: [6, 18] },
  { version: 3, size: 29, dataCodewords: 55, ecCodewordsPerBlock: 15, group1Blocks: 1, group1Data: 55, group2Blocks: 0, group2Data: 0, alignment: [6, 22] },
  { version: 4, size: 33, dataCodewords: 80, ecCodewordsPerBlock: 20, group1Blocks: 1, group1Data: 80, group2Blocks: 0, group2Data: 0, alignment: [6, 26] },
  { version: 5, size: 37, dataCodewords: 108, ecCodewordsPerBlock: 26, group1Blocks: 1, group1Data: 108, group2Blocks: 0, group2Data: 0, alignment: [6, 30] },
  { version: 6, size: 41, dataCodewords: 136, ecCodewordsPerBlock: 18, group1Blocks: 2, group1Data: 68, group2Blocks: 0, group2Data: 0, alignment: [6, 34] },
  { version: 7, size: 45, dataCodewords: 156, ecCodewordsPerBlock: 20, group1Blocks: 2, group1Data: 78, group2Blocks: 0, group2Data: 0, alignment: [6, 22, 38] },
  { version: 8, size: 49, dataCodewords: 194, ecCodewordsPerBlock: 24, group1Blocks: 2, group1Data: 97, group2Blocks: 0, group2Data: 0, alignment: [6, 24, 42] },
  { version: 9, size: 53, dataCodewords: 232, ecCodewordsPerBlock: 30, group1Blocks: 2, group1Data: 116, group2Blocks: 0, group2Data: 0, alignment: [6, 26, 46] },
  { version: 10, size: 57, dataCodewords: 274, ecCodewordsPerBlock: 18, group1Blocks: 2, group1Data: 68, group2Blocks: 2, group2Data: 69, alignment: [6, 28, 50] },
]

// Version information (18-bit BCH(18,6)) for versions 7..10
const VERSION_INFO_BITS: Record<number, number> = {
  7: 0x07c94,
  8: 0x085bc,
  9: 0x09a99,
  10: 0x0a4d3,
}

// GF(256) with primitive polynomial 0x11d (285)
const GF_EXP = new Uint8Array(512)
const GF_LOG = new Uint8Array(256)
{
  let x = 1
  for (let i = 0; i < 255; i++) {
    GF_EXP[i] = x
    GF_LOG[x] = i
    x <<= 1
    if (x & 0x100) x ^= 0x11d
  }
  for (let i = 255; i < 512; i++) {
    GF_EXP[i] = GF_EXP[i - 255]
  }
}

function gfMul(a: number, b: number): number {
  if (a === 0 || b === 0) return 0
  return GF_EXP[GF_LOG[a] + GF_LOG[b]]
}

function rsGeneratorPoly(degree: number): Uint8Array {
  let poly = new Uint8Array([1])
  for (let i = 0; i < degree; i++) {
    const next = new Uint8Array(poly.length + 1)
    const factor = GF_EXP[i]
    for (let j = 0; j < poly.length; j++) {
      next[j] ^= poly[j]
      next[j + 1] ^= gfMul(poly[j], factor)
    }
    poly = next
  }
  return poly
}

function rsRemainder(data: Uint8Array, ecCount: number): Uint8Array {
  const gen = rsGeneratorPoly(ecCount)
  const res = new Uint8Array(ecCount)
  for (let i = 0; i < data.length; i++) {
    const factor = data[i] ^ res[0]
    res.copyWithin(0, 1)
    res[ecCount - 1] = 0
    for (let j = 0; j < ecCount; j++) {
      res[j] ^= gfMul(gen[j + 1], factor)
    }
  }
  return res
}

function encodeDataCodewords(bytes: Uint8Array, spec: VersionSpec): Uint8Array {
  const countBits = spec.version <= 9 ? 8 : 16
  const bits: number[] = []
  const pushBits = (val: number, len: number) => {
    for (let i = len - 1; i >= 0; i--) {
      bits.push((val >>> i) & 1)
    }
  }

  // Mode 0100 = Byte
  pushBits(0b0100, 4)
  pushBits(bytes.length, countBits)
  for (const b of bytes) pushBits(b, 8)

  const capacityBits = spec.dataCodewords * 8
  const terminator = Math.min(4, capacityBits - bits.length)
  for (let i = 0; i < terminator; i++) bits.push(0)
  while (bits.length % 8 !== 0) bits.push(0)

  const out = new Uint8Array(spec.dataCodewords)
  let byteIdx = 0
  for (let i = 0; i < bits.length; i += 8) {
    let v = 0
    for (let j = 0; j < 8; j++) v = (v << 1) | bits[i + j]
    out[byteIdx++] = v
  }

  const pads = [0xec, 0x11]
  let p = 0
  while (byteIdx < spec.dataCodewords) {
    out[byteIdx++] = pads[p]
    p ^= 1
  }
  return out
}

function buildCodewordStream(data: Uint8Array, spec: VersionSpec): number[] {
  const blocks: Uint8Array[] = []
  const ecBlocks: Uint8Array[] = []
  let offset = 0

  for (let b = 0; b < spec.group1Blocks; b++) {
    const slice = data.subarray(offset, offset + spec.group1Data)
    blocks.push(slice)
    ecBlocks.push(rsRemainder(slice, spec.ecCodewordsPerBlock))
    offset += spec.group1Data
  }
  for (let b = 0; b < spec.group2Blocks; b++) {
    const slice = data.subarray(offset, offset + spec.group2Data)
    blocks.push(slice)
    ecBlocks.push(rsRemainder(slice, spec.ecCodewordsPerBlock))
    offset += spec.group2Data
  }

  const stream: number[] = []
  const maxDataLen = Math.max(spec.group1Data, spec.group2Data)
  for (let i = 0; i < maxDataLen; i++) {
    for (const blk of blocks) {
      if (i < blk.length) stream.push(blk[i])
    }
  }
  for (let i = 0; i < spec.ecCodewordsPerBlock; i++) {
    for (const ec of ecBlocks) {
      stream.push(ec[i])
    }
  }
  return stream
}

function drawFinder(grid: (boolean | null)[][], isFunc: boolean[][], r0: number, c0: number) {
  const size = grid.length
  for (let dr = -1; dr <= 7; dr++) {
    for (let dc = -1; dc <= 7; dc++) {
      const r = r0 + dr
      const c = c0 + dc
      if (r < 0 || c < 0 || r >= size || c >= size) continue
      const inOuter = dr >= 0 && dr <= 6 && dc >= 0 && dc <= 6
      const dark =
        inOuter &&
        (dr === 0 ||
          dr === 6 ||
          dc === 0 ||
          dc === 6 ||
          (dr >= 2 && dr <= 4 && dc >= 2 && dc <= 4))
      grid[r][c] = dark
      isFunc[r][c] = true
    }
  }
}

function drawAlignment(grid: (boolean | null)[][], isFunc: boolean[][], cr: number, cc: number) {
  for (let dr = -2; dr <= 2; dr++) {
    for (let dc = -2; dc <= 2; dc++) {
      const dark = Math.abs(dr) === 2 || Math.abs(dc) === 2 || (dr === 0 && dc === 0)
      grid[cr + dr][cc + dc] = dark
      isFunc[cr + dr][cc + dc] = true
    }
  }
}

function formatInfoBits(mask: number): number {
  // EC level L = 01
  const data = (0b01 << 3) | (mask & 0x7)
  let rem = data << 10
  for (let i = 14; i >= 10; i--) {
    if ((rem >>> i) & 1) rem ^= 0x537 << (i - 10)
  }
  return ((data << 10) | rem) ^ 0x5412
}

function applyMaskAndFormat(
  base: (boolean | null)[][],
  isFunc: boolean[][],
  mask: number,
  version: number
): boolean[][] {
  const size = base.length
  const out: boolean[][] = Array.from({ length: size }, (_, r) =>
    Array.from({ length: size }, (_, c) => {
      const v = !!base[r][c]
      if (isFunc[r][c]) return v
      let flip = false
      switch (mask) {
        case 0: flip = (r + c) % 2 === 0; break
        case 1: flip = r % 2 === 0; break
        case 2: flip = c % 3 === 0; break
        case 3: flip = (r + c) % 3 === 0; break
        case 4: flip = (Math.floor(r / 2) + Math.floor(c / 3)) % 2 === 0; break
        case 5: flip = ((r * c) % 2) + ((r * c) % 3) === 0; break
        case 6: flip = (((r * c) % 2) + ((r * c) % 3)) % 2 === 0; break
        case 7: flip = (((r + c) % 2) + ((r * c) % 3)) % 2 === 0; break
      }
      return flip ? !v : v
    })
  )

  const fmt = formatInfoBits(mask)
  for (let i = 0; i < 15; i++) {
    const bit = ((fmt >>> i) & 1) === 1
    if (i < 6) out[8][i] = bit
    else if (i < 8) out[8][i + 1] = bit
    else out[8][size - 15 + i] = bit

    if (i < 8) out[size - 1 - i][8] = bit
    else if (i < 9) out[15 - i][8] = bit
    else out[14 - i][8] = bit
  }
  out[size - 8][8] = true

  if (version >= 7) {
    const vbits = VERSION_INFO_BITS[version]
    for (let i = 0; i < 18; i++) {
      const bit = ((vbits >>> i) & 1) === 1
      const a = Math.floor(i / 3)
      const b = i % 3
      out[a][size - 11 + b] = bit
      out[size - 11 + b][a] = bit
    }
  }
  return out
}

function penaltyScore(m: boolean[][]): number {
  const n = m.length
  let score = 0

  // Rule 1: 5+ consecutive same-color modules
  for (let r = 0; r < n; r++) {
    let run = 1
    for (let c = 1; c < n; c++) {
      if (m[r][c] === m[r][c - 1]) {
        run++
      } else {
        if (run >= 5) score += 3 + (run - 5)
        run = 1
      }
    }
    if (run >= 5) score += 3 + (run - 5)
  }
  for (let c = 0; c < n; c++) {
    let run = 1
    for (let r = 1; r < n; r++) {
      if (m[r][c] === m[r - 1][c]) {
        run++
      } else {
        if (run >= 5) score += 3 + (run - 5)
        run = 1
      }
    }
    if (run >= 5) score += 3 + (run - 5)
  }

  // Rule 2: 2x2 blocks
  for (let r = 0; r < n - 1; r++) {
    for (let c = 0; c < n - 1; c++) {
      const v = m[r][c]
      if (m[r][c + 1] === v && m[r + 1][c] === v && m[r + 1][c + 1] === v) score += 3
    }
  }

  // Rule 3: finder-like patterns
  const p1 = [true, false, true, true, true, false, true, false, false, false, false]
  const p2 = [false, false, false, false, true, false, true, true, true, false, true]
  for (let r = 0; r < n; r++) {
    for (let c = 0; c <= n - 11; c++) {
      let m1 = true
      let m2 = true
      for (let k = 0; k < 11; k++) {
        if (m[r][c + k] !== p1[k]) m1 = false
        if (m[r][c + k] !== p2[k]) m2 = false
      }
      if (m1 || m2) score += 40
    }
  }
  for (let c = 0; c < n; c++) {
    for (let r = 0; r <= n - 11; r++) {
      let m1 = true
      let m2 = true
      for (let k = 0; k < 11; k++) {
        if (m[r + k][c] !== p1[k]) m1 = false
        if (m[r + k][c] !== p2[k]) m2 = false
      }
      if (m1 || m2) score += 40
    }
  }

  // Rule 4: dark ratio
  let dark = 0
  for (let r = 0; r < n; r++) {
    for (let c = 0; c < n; c++) {
      if (m[r][c]) dark++
    }
  }
  const pct = (dark * 100) / (n * n)
  const k = Math.floor(Math.abs(pct - 50) / 5)
  score += k * 10
  return score
}

export function encodeQrMatrix(text: string): boolean[][] {
  const bytes = new TextEncoder().encode(text)
  const spec = VERSIONS_L.find((v) => {
    const countBytes = v.version <= 9 ? 1 : 2
    // 4-bit mode + 8/16-bit length + payload bytes <= dataCodewords * 8
    return 4 + countBytes * 8 + bytes.length * 8 <= v.dataCodewords * 8
  })
  if (!spec) {
    throw new Error(`QR payload too long (${bytes.length} bytes)`)
  }

  const size = spec.size
  const grid: (boolean | null)[][] = Array.from({ length: size }, () => Array(size).fill(null))
  const isFunc: boolean[][] = Array.from({ length: size }, () => Array(size).fill(false))

  drawFinder(grid, isFunc, 0, 0)
  drawFinder(grid, isFunc, 0, size - 7)
  drawFinder(grid, isFunc, size - 7, 0)

  // Timing patterns
  for (let i = 8; i < size - 8; i++) {
    const dark = i % 2 === 0
    grid[6][i] = dark
    isFunc[6][i] = true
    grid[i][6] = dark
    isFunc[i][6] = true
  }

  // Alignment patterns
  for (const r of spec.alignment) {
    for (const c of spec.alignment) {
      if ((r === 6 && c === 6) || (r === 6 && c === size - 7) || (r === size - 7 && c === 6)) {
        continue
      }
      drawAlignment(grid, isFunc, r, c)
    }
  }

  // Reserve format & version areas
  for (let i = 0; i < 9; i++) {
    if (i !== 6) {
      isFunc[8][i] = true
      isFunc[i][8] = true
    }
  }
  for (let i = 0; i < 8; i++) {
    isFunc[8][size - 1 - i] = true
    isFunc[size - 1 - i][8] = true
  }
  grid[size - 8][8] = true
  isFunc[size - 8][8] = true

  if (spec.version >= 7) {
    for (let i = 0; i < 6; i++) {
      for (let j = 0; j < 3; j++) {
        isFunc[i][size - 11 + j] = true
        isFunc[size - 11 + j][i] = true
      }
    }
  }

  const data = encodeDataCodewords(bytes, spec)
  const stream = buildCodewordStream(data, spec)
  const bitStream: boolean[] = []
  for (const cw of stream) {
    for (let b = 7; b >= 0; b--) bitStream.push(((cw >>> b) & 1) === 1)
  }

  let bitIdx = 0
  let up = true
  for (let right = size - 1; right >= 1; right -= 2) {
    if (right === 6) right = 5
    for (let step = 0; step < size; step++) {
      const r = up ? size - 1 - step : step
      for (let dx = 0; dx < 2; dx++) {
        const c = right - dx
        if (!isFunc[r][c]) {
          grid[r][c] = bitIdx < bitStream.length ? bitStream[bitIdx++] : false
        }
      }
    }
    up = !up
  }

  let best = applyMaskAndFormat(grid, isFunc, 0, spec.version)
  let bestScore = penaltyScore(best)
  for (let mask = 1; mask < 8; mask++) {
    const candidate = applyMaskAndFormat(grid, isFunc, mask, spec.version)
    const s = penaltyScore(candidate)
    if (s < bestScore) {
      bestScore = s
      best = candidate
    }
  }
  return best
}
