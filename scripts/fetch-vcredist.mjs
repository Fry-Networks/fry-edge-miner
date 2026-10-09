// Fetches + verifies the pinned Microsoft-signed VC++ 2015-2022 x64 redist
// (Windows only). All network and verification logic lives in the .ps1.
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

if (process.platform !== 'win32') {
  console.log('fetch-vcredist: skipped (non-Windows)');
  process.exit(0);
}

const ps1 = fileURLToPath(new URL('./fetch-vcredist.ps1', import.meta.url));
// Windows PowerShell 5.1 spawned from a PowerShell 7 process inherits PS7's
// PSModulePath and then cannot autoload Microsoft.PowerShell.Utility
// (Get-FileHash). Drop it (env keys are case-insensitive on Windows) so 5.1
// rebuilds its default module path.
const childEnv = {};
for (const k of Object.keys(process.env)) {
  if (k.toUpperCase() !== 'PSMODULEPATH') childEnv[k] = process.env[k];
}
const r = spawnSync(
  'powershell',
  ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', ps1],
  { stdio: 'inherit', env: childEnv },
);
if (r.error) {
  console.error(`fetch-vcredist: failed to start powershell: ${r.error.message}`);
  process.exit(1);
}
process.exit(r.status ?? 1);
