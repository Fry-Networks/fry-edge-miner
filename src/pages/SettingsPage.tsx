import { useEffect, useState } from 'react'
import { Info, Key, Monitor, Shield, Wallet } from 'lucide-react'
import type { LucideIcon } from 'lucide-react'
import Btn from '../components/primitives/Btn'
import CopyField from '../components/primitives/CopyField'
import Divider from '../components/primitives/Divider'
import Lbl from '../components/primitives/Lbl'
import SettingRow from '../components/SettingRow'
import Tog from '../components/primitives/Tog'
import type { FemConfig } from '../lib/types'
import { invokeWithFallback } from '../lib/tauri'
import { useRewards } from '../hooks/useRewards'
import { APP_VERSION } from '../lib/version'

interface SettingSectionProps {
  Icon: LucideIcon
  ico: string
  label: string
  children: React.ReactNode
}

function SettingSection({ Icon, ico, label, children }: SettingSectionProps) {
  return (
    <div
      style={{
        background: 'var(--s1)',
        border: '1px solid var(--b0)',
        borderRadius: 'var(--rad)',
        padding: '18px 20px',
        marginBottom: 12
      }}
    >
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 14 }}>
        <Icon size={15} color={ico} />
        <div
          style={{
            fontFamily: 'var(--fh)',
            fontWeight: 700,
            fontSize: 13,
            color: 'var(--txt)',
            letterSpacing: '.03em'
          }}
        >
          {label}
        </div>
      </div>
      {children}
    </div>
  )
}

interface SettingsPageProps {
  deviceName?: string
  deregister: () => Promise<void>
}

export default function SettingsPage({ deviceName = 'nimble-swift-wolf', deregister }: SettingsPageProps) {
  const [boot, setBoot] = useState(true)
  const [tray, setTray] = useState(true)
  const [auto, setAuto] = useState(true)
  const [notif, setNotif] = useState(true)
  const [config, setConfig] = useState<FemConfig | null>(null)
  const { rewards } = useRewards()
  const summary = rewards.summary

  useEffect(() => {
    invokeWithFallback<FemConfig>('get_settings', undefined, {
      miner_key: 'FEM-b9e489c8a32d5547bbb7c363baaf733e',
      wallet_address: 'OGHVJYWQXOOPZG2OLBIRFNTBF3H3276DDTKYYZUA6G4NUMF2RGYXNTMIRE',
      integrations_enabled: {},
      api_base_url: 'https://hardwareapi.frynetworks.com'
    }).then((cfg) => {
      setConfig(cfg)
      if (cfg) {
        if (cfg.start_on_boot !== undefined) setBoot(cfg.start_on_boot)
        if (cfg.minimize_to_tray !== undefined) setTray(cfg.minimize_to_tray)
        if (cfg.auto_update !== undefined) setAuto(cfg.auto_update)
        if (cfg.notifications !== undefined) setNotif(cfg.notifications)
      }
    })
  }, [])

  const savePrefs = async (next?: { boot?: boolean; tray?: boolean; auto?: boolean; notif?: boolean }) => {
    try {
      await invokeWithFallback('save_settings', {
        settings: {
          start_on_boot: next?.boot ?? boot,
          minimize_to_tray: next?.tray ?? tray,
          auto_update: next?.auto ?? auto,
          notifications: next?.notif ?? notif
        }
      }, undefined)
    } catch (e) {
      console.warn('save_settings failed:', e)
    }
  }

  return (
    <div className="sc" style={{ padding: '20px 24px', overflowY: 'auto', height: '100%' }}>
      <SettingSection Icon={Key} ico="var(--red)" label="Device Identity">
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 14, marginBottom: 12 }}>
          <div>
            <Lbl sx={{ marginBottom: 5 }}>Display Name</Lbl>
            <div
              style={{
                fontFamily: 'var(--fl)',
                fontWeight: 700,
                fontSize: 15,
                color: 'var(--teal)',
                letterSpacing: '.03em'
              }}
            >
              {deviceName}
            </div>
          </div>
          <div>
            <Lbl sx={{ marginBottom: 6 }}>Registration</Lbl>
            <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
              <Shield size={13} color="var(--teal)" strokeWidth={2.5} />
              <span style={{ fontFamily: 'var(--fb)', fontSize: 13, color: 'var(--teal)' }}>Registered</span>
              <span style={{ fontFamily: 'var(--fm)', fontSize: 10, color: 'var(--t2)' }}>{summary ? `${summary.stake_label}${summary.stake_label.toLowerCase().includes('stake') ? '' : ' stake'}` : '—'}</span>
            </div>
          </div>
        </div>
        <Lbl sx={{ marginBottom: 5 }}>Miner Key</Lbl>
        <CopyField val={config?.miner_key ?? 'FEM-b9e489c8a32d5547bbb7c363baaf733e'} />
      </SettingSection>

      <SettingSection Icon={Wallet} ico="var(--amb)" label="Reward Wallet">
        <Lbl sx={{ marginBottom: 6 }}>Algorand Address</Lbl>
        <CopyField
          val={
            config?.wallet_address ??
            'OGHVJYWQXOOPZG2OLBIRFNTBF3H3276DDTKYYZUA6G4NUMF2RGYXNTMIRE'
          }
        />
        <div style={{ display: 'flex', alignItems: 'center', gap: 7, marginTop: 10 }}>
          <Shield size={12} color="var(--teal)" />
          <span style={{ fontFamily: 'var(--fb)', fontSize: 12, color: 'var(--t1)' }}>{summary ? `${summary.stake_label}${summary.stake_label.toLowerCase().includes('stake') ? '' : ' stake'} active` : 'Stake info unavailable'}</span>
          <span style={{ fontFamily: 'var(--fm)', fontSize: 12, color: 'var(--teal)' }}>{summary ? `${summary.stake_multiplier.toFixed(1)}×` : '—'}</span>
        </div>
      </SettingSection>

      <SettingSection Icon={Monitor} ico="var(--blu)" label="Preferences">
        <SettingRow label="Start on boot" sub="Launch FEM when Windows starts">
          <Tog
            checked={boot}
            onChange={(v) => {
              setBoot(v)
              savePrefs({ boot: v })
            }}
          />
        </SettingRow>
        <SettingRow label="Minimize to tray" sub="Keep running in the system tray when closed">
          <Tog
            checked={tray}
            onChange={(v) => {
              setTray(v)
              savePrefs({ tray: v })
            }}
          />
        </SettingRow>
        <SettingRow label="Auto-update" sub="Automatically apply FEM updates when available">
          <Tog
            checked={auto}
            onChange={(v) => {
              setAuto(v)
              savePrefs({ auto: v })
            }}
          />
        </SettingRow>
        <SettingRow label="Notifications" sub="Desktop notifications for reward events and alerts">
          <Tog
            checked={notif}
            onChange={(v) => {
              setNotif(v)
              savePrefs({ notif: v })
            }}
          />
        </SettingRow>
      </SettingSection>

      <SettingSection Icon={Info} ico="var(--t2)" label="About">
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr', gap: 14, marginBottom: 12 }}>
          {[
            ['Version', APP_VERSION],
            ['Platform', 'Windows x64'],
            ['Tauri', '2.1.0']
          ].map(([l, v]) => (
            <div key={l}>
              <Lbl sx={{ marginBottom: 4 }}>{l}</Lbl>
              <span style={{ fontFamily: 'var(--fm)', fontSize: 13, color: 'var(--txt)' }}>{v}</span>
            </div>
          ))}
        </div>
        <Divider sx={{ marginBottom: 10 }} />
        <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
          <span style={{ fontFamily: 'var(--fb)', fontSize: 12, color: 'var(--t2)' }}>Fry Networks</span>
          <span style={{ color: 'var(--b2)' }}>·</span>
          <span
            style={{
              fontFamily: 'var(--fb)',
              fontSize: 12,
              color: 'var(--red)',
              cursor: 'pointer'
            }}
          >
            frynetworks.com
          </span>
          <span style={{ color: 'var(--b2)' }}>·</span>
          <span
            style={{
              fontFamily: 'var(--fb)',
              fontSize: 12,
              color: 'var(--red)',
              cursor: 'pointer'
            }}
          >
            Discord
          </span>
        </div>
      </SettingSection>

      <div style={{ display: 'flex', gap: 8, marginTop: 8 }}>
        <Btn v="g" onClick={() => { if (window.confirm('Deregister this device? This cannot be undone.')) deregister() }} >
          Deregister
        </Btn>
      </div>
    </div>
  )
}
