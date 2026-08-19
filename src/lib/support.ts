// Where users take problems with the community SDK integrations. These builds
// are not covered by a partner support contract, so every surface that shows
// an SDK integration also has to say where to report it — otherwise the only
// visible channel is the generic Discord link buried in Settings > About.
//
// No other channel name exists anywhere in this repo (the Settings footer
// links "Discord" with no channel), so this constant is the single source.

export const SDK_DISCORD_CHANNEL = '#fry-edge-miner-testing'

export const SDK_REPORT_LINE = `Report issues in Discord → ${SDK_DISCORD_CHANNEL}`

/** Shown when an official integration is installed but switched off. */
export const OFFICIAL_DISABLED_WARNING =
  'Disabling an official integration reduces your base reward proportion.'
