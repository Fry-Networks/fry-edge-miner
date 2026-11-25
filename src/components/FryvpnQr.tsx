import { encodeQrMatrix } from '../lib/qr'

interface FryvpnQrProps {
  value: string
  size?: number
}

export default function FryvpnQr({ value, size = 104 }: FryvpnQrProps) {
  const matrix = encodeQrMatrix(value)
  const quiet = 4
  const dim = matrix.length + quiet * 2
  const parts: string[] = []
  for (let r = 0; r < matrix.length; r++) {
    for (let c = 0; c < matrix.length; c++) {
      if (matrix[r][c]) {
        parts.push(`M${c + quiet},${r + quiet}h1v1h-1z`)
      }
    }
  }

  return (
    <svg
      role="img"
      aria-label="QR code for Fry dVPN node wallet"
      width={size}
      height={size}
      viewBox={`0 0 ${dim} ${dim}`}
      shapeRendering="crispEdges"
      style={{
        display: 'block',
        background: '#ffffff',
        borderRadius: 4,
        border: '1px solid var(--b1)',
        flexShrink: 0,
      }}
    >
      <rect width={dim} height={dim} fill="#ffffff" />
      <path d={parts.join('')} fill="#000000" />
    </svg>
  )
}
