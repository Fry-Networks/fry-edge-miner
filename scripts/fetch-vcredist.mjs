// Fetches + verifies the pinned Microsoft-signed VC++ 2015-2022 x64 redist
// (Windows only). All network and verification logic lives in the .ps1.
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

if (process.platform !== 'win32') {
  console.log('fetch-vcredist: skipped (non-Windows)');
  process.exit(0);
}

const ps1 = fileURLToPath(new URL('./fetch-vcredist.ps1', import.meta.url));
const r = spawnSync(
  'powershell',
  ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', ps1],
  { stdio: 'inherit' },
);
process.exit(r.status ?? 1);
