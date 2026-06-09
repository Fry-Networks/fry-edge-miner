# Fry Edge Miner (FEM)

Multi-integration DePIN client that manages partner software (Mysterium, Presearch, SpaceAcres, Diiisco, AEM) with PoC-based rewards on the Fry Networks platform.

## Architecture

**Tauri v2** — Rust backend + React frontend.

- **Rust backend** manages partner processes via a process supervisor, reports PoC data every 10 minutes (144 slots/day), and communicates with the FryNetworks hardwareapi.
- **React frontend** provides a dark-themed dashboard showing integration health, reward proportions, PoC slot activity, and device configuration.
- **IPC bridge** connects frontend to backend via Tauri commands with typed hooks.

### Module Structure

```
src-tauri/src/
├── api/         — HTTP client + typed endpoints for hardwareapi
├── commands/    — Tauri IPC command handlers
├── config/      — Configuration store, miner key generation, wallet
├── integrations/— Partner integration trait, registry, 5 implementations
├── poc/         — PoC reporter, gate evaluation, hardware probing
├── supervisor/  — Child process management + health checks
└── main.rs      — App initialization + state wiring
```

## Build

**Prerequisites:** Rust 1.78+, Node 20+, cargo-tauri CLI

```bash
npm install
cargo tauri dev     # development (hot-reload frontend)
cargo tauri build   # production installer (.msi / .exe)
```

## Reward Mechanics

FEM earns rewards proportional to active integrations:

- 5 integration slots (20% each): mysterium, presearch, diiisco, space_acres, aem
- `proportion = active_integrations / total_integrations` (0.0 to 1.0)
- Daily reward = `base_verified_reward × proportion`
- PoC data submitted every 10 minutes (slot-based, 144 slots/day)
- Proportion is clamped to [0, 1] server-side to prevent inflation

## Phase Status

- [x] Phase 1: Tauri v2 scaffold
- [x] Phase 3: Core Rust modules (config, API client, supervisor, PoC reporter, IPC commands)
- [x] Phase 4: Partner integrations (SpaceAcres full, others framework-ready)
- [x] Phase 5: React frontend (Dashboard, Integrations, Rewards, Settings, Updates)
- [x] Phase 6: Build config (CI/CD, Tauri updater, NSIS installer)
- [ ] Phase 7: Production deployment + operator testing

## License

MIT — Fry Networks
