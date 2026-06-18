const NN = [
  { x: 12, y: 8 }, { x: 50, y: 5 }, { x: 88, y: 12 }, { x: 25, y: 22 },
  { x: 72, y: 18 }, { x: 40, y: 35 }, { x: 85, y: 40 }, { x: 10, y: 45 },
  { x: 60, y: 50 }, { x: 30, y: 60 }, { x: 78, y: 62 }, { x: 18, y: 75 },
  { x: 55, y: 72 }, { x: 90, y: 78 }, { x: 42, y: 88 }, { x: 68, y: 92 },
  { x: 8, y: 92 }, { x: 50, y: 95 }, { x: 35, y: 50 }, { x: 65, y: 30 }
]

const NE = [
  [0, 3], [0, 1], [1, 4], [1, 5], [2, 4], [3, 5], [4, 6], [4, 19],
  [5, 18], [5, 8], [6, 10], [7, 3], [7, 11], [8, 12], [8, 18], [9, 11],
  [9, 12], [10, 13], [11, 16], [12, 14], [13, 15], [14, 17], [15, 17],
  [16, 14], [18, 19], [19, 6]
]

export default function NetSVG() {
  return (
    <svg
      viewBox="0 0 100 100"
      preserveAspectRatio="xMidYMid slice"
      style={{ position: 'absolute', inset: 0, width: '100%', height: '100%', opacity: 0.2 }}
    >
      {NE.map(([a, b], i) => (
        <line
          key={i}
          x1={NN[a].x}
          y1={NN[a].y}
          x2={NN[b].x}
          y2={NN[b].y}
          stroke="#e5271c"
          strokeWidth=".4"
          style={{ animation: `ep ${2 + ((i * 0.17) % 3)}s ease-in-out infinite alternate` }}
        />
      ))}
      {NN.map((n, i) => (
        <circle
          key={i}
          cx={n.x}
          cy={n.y}
          r={[0, 1, 8].includes(i) ? 2 : 1}
          fill="#e5271c"
          style={{ animation: `np ${1.8 + ((i * 0.21) % 2)}s ease-in-out infinite alternate` }}
        />
      ))}
    </svg>
  )
}
