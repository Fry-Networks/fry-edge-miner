import { useEffect, useRef, useState } from 'react'
import { ArrowRight, Check, RefreshCw } from 'lucide-react'
import Btn from '../components/primitives/Btn'
import Divider from '../components/primitives/Divider'
import Lbl from '../components/primitives/Lbl'
import { useDevice } from '../hooks/useDevice'
import { makeName } from '../lib/names'
import { extractErrorMessage } from '../lib/error'

interface Step3Props {
  minerKey: string | null
  walletAddress: string
  onDone: (name: string) => void
}

export default function Step3Install({ minerKey, walletAddress, onDone }: Step3Props) {
  const { register } = useDevice()
  const [status, setStatus] = useState<'installing' | 'done' | 'error'>('installing')
  const [errorMsg, setErrorMsg] = useState('')
  // Bumped by the Retry button — registration only re-runs when this changes,
  // since none of the other effect deps change on retry.
  const [attempt, setAttempt] = useState(0)
  const deviceName = useRef(makeName()).current

  useEffect(() => {
    let cancelled = false
    // B8: hard cap — the spinner must never run forever. The backend's own
    // retry ladder tops out well under this, so a hit here means a hung IPC
    // or network path; surface a distinct timeout state with Retry.
    const REGISTER_TIMEOUT_MS = 120_000
    let timer: ReturnType<typeof setTimeout> | undefined
    const timeout = new Promise<never>((_, reject) => {
      timer = setTimeout(
        () =>
          reject(
            new Error(
              'Registration timed out after 120 seconds.\nThe request may still be processing — wait a moment, then press Retry. If it keeps timing out, check your connection (some networks block *.frynetworks.com).'
            )
          ),
        REGISTER_TIMEOUT_MS
      )
    })
    Promise.race([register(walletAddress, minerKey ?? undefined, deviceName), timeout])
      .then(() => {
        if (!cancelled) setStatus('done')
      })
      .catch((e: unknown) => {
        if (!cancelled) {
          setErrorMsg(extractErrorMessage(e))
          setStatus('error')
        }
      })
      .finally(() => {
        if (timer) clearTimeout(timer)
      })
    return () => {
      cancelled = true
      if (timer) clearTimeout(timer)
    }
  }, [register, walletAddress, minerKey, deviceName, attempt])

  if (status === 'done') {
    return (
      <div
        className="fu"
        style={{
          flex: 1,
          display: 'flex',
          flexDirection: 'column',
          alignItems: 'center',
          justifyContent: 'center',
          padding: '36px 44px',
          textAlign: 'center'
        }}
      >
        <div
          style={{
            width: 68,
            height: 68,
            borderRadius: '50%',
            background: 'var(--tealg)',
            border: '2px solid var(--teal)',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            marginBottom: 20,
            animation: 'pop .5s cubic-bezier(.34,1.56,.64,1) both'
          }}
        >
          <Check size={30} color="var(--teal)" strokeWidth={2} />
        </div>
        <div
          style={{
            fontFamily: 'var(--fh)',
            fontWeight: 700,
            fontSize: 26,
            color: 'var(--txt)',
            marginBottom: 6
          }}
        >
          Installation Complete
        </div>
        <div
          style={{
            fontFamily: 'var(--fb)',
            fontSize: 13,
            color: 'var(--t1)',
            marginBottom: 24,
            maxWidth: 320
          }}
        >
          Your device is registered. Rewards begin accruing in the next PoC cycle.
        </div>
        <div
          style={{
            background: 'var(--s2)',
            border: '1px solid var(--b1)',
            borderRadius: 'var(--rad)',
            padding: '16px 20px',
            marginBottom: 24,
            width: '100%',
            maxWidth: 400
          }}
        >
          <div
            style={{
              fontFamily: 'var(--fl)',
              fontWeight: 700,
              fontSize: 20,
              color: 'var(--teal)',
              letterSpacing: '.04em',
              marginBottom: 4
            }}
          >
            {deviceName}
          </div>
          <div style={{ fontFamily: 'var(--fb)', fontSize: 11, color: 'var(--t2)', marginBottom: 12 }}>Your device&apos;s display name</div>
          <Divider sx={{ marginBottom: 10 }} />
          <Lbl sx={{ marginBottom: 5 }}>Miner Key</Lbl>
          <code
            style={{
              fontFamily: 'var(--fm)',
              fontSize: 11,
              color: 'var(--txt)',
              lineHeight: 1.5,
              display: 'block',
              wordBreak: 'break-all'
            }}
          >
            {minerKey ?? 'Auto-generated'}
          </code>
        </div>
        <Btn
          v="t"
          onClick={() => onDone(deviceName)}
          sx={{ fontSize: 14, padding: '10px 24px' }}
        >
          Open Dashboard <ArrowRight size={14} />
        </Btn>
      </div>
    )
  }

  if (status === 'error') {
    return (
      <div
        className="fu"
        style={{
          flex: 1,
          display: 'flex',
          flexDirection: 'column',
          alignItems: 'center',
          justifyContent: 'center',
          padding: '36px 44px',
          textAlign: 'center'
        }}
      >
        <div
          style={{
            fontFamily: 'var(--fh)',
            fontWeight: 700,
            fontSize: 22,
            color: 'var(--red)',
            marginBottom: 8
          }}
        >
          Registration Failed
        </div>
        <div
          style={{
            fontFamily: 'var(--fb)',
            fontSize: 13,
            color: 'var(--t1)',
            marginBottom: 24,
            maxWidth: 420,
            whiteSpace: 'pre-wrap',
            wordBreak: 'break-word',
            textAlign: 'left'
          }}
        >
          {errorMsg}
        </div>
        <Btn
          v="p"
          onClick={() => {
            setStatus('installing')
            setAttempt((a) => a + 1)
          }}
          sx={{ fontSize: 14, padding: '10px 24px' }}
        >
          <RefreshCw size={14} style={{ marginRight: 6 }} />
          Retry
        </Btn>
      </div>
    )
  }

  return (
    <div
      className="fu"
      style={{
        flex: 1,
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        justifyContent: 'center',
        padding: '36px 44px',
        textAlign: 'center'
      }}
    >
      <div
        style={{
          width: 40,
          height: 40,
          borderRadius: '50%',
          border: '3px solid var(--b1)',
          borderTopColor: 'var(--teal)',
          animation: 'spin 1s linear infinite',
          marginBottom: 20
        }}
      />
      <div
        style={{
          fontFamily: 'var(--fh)',
          fontWeight: 700,
          fontSize: 20,
          color: 'var(--txt)',
          marginBottom: 6
        }}
      >
        Registering Device…
      </div>
      <div style={{ fontFamily: 'var(--fb)', fontSize: 13, color: 'var(--t1)' }}>
        Please wait while we register your device with Fry Networks.
      </div>
    </div>
  )
}
