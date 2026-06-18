export const ADJS = [
  'amber', 'bold', 'brave', 'calm', 'clever', 'cool', 'daring', 'deft', 'fast',
  'fierce', 'firm', 'fleet', 'fresh', 'grand', 'hardy', 'keen', 'lean', 'light',
  'lucky', 'nimble', 'noble', 'quiet', 'rapid', 'rare', 'sharp', 'sleek', 'smart',
  'solid', 'steady', 'swift', 'warm', 'wise'
]

export const ANIS = [
  'badger', 'bear', 'bobcat', 'cobra', 'condor', 'crane', 'crow', 'deer', 'eagle',
  'falcon', 'ferret', 'fox', 'hawk', 'heron', 'jaguar', 'lynx', 'otter', 'owl',
  'puma', 'raven', 'robin', 'seal', 'shark', 'sparrow', 'stag', 'wolf', 'wren'
]

export function makeName(): string {
  const a = ADJS[Math.floor(Math.random() * ADJS.length)]
  const b = ADJS[Math.floor(Math.random() * ADJS.length)]
  const c = ANIS[Math.floor(Math.random() * ANIS.length)]
  return `${a}-${b}-${c}`
}
