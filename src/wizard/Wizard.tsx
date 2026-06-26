import { useState } from 'react'
import Step0Welcome from './Step0Welcome'
import Step1Device from './Step1Device'
import Step2Review from './Step2Review'
import Step3Install from './Step3Install'
import WizPanel from './WizPanel'

interface WizardProps {
  onDone: (name: string) => void
}

export default function Wizard({ onDone }: WizardProps) {
  const [step, setStep] = useState(0)
  const [minerKey, setMinerKey] = useState<string | null>(null)
  const [walletAddress, setWalletAddress] = useState('')
  const next = () => setStep((s) => s + 1)
  const back = () => setStep((s) => s - 1)
  const goToStep1 = (key: string, wallet: string) => {
    setMinerKey(key)
    setWalletAddress(wallet)
    next()
  }
  return (
    <div
      style={{
        display: 'flex',
        height: '100%',
        overflow: 'hidden',
        background: 'var(--bg)',
        fontFamily: 'var(--fb)'
      }}
    >
      <WizPanel step={step} />
      {step === 0 && <Step0Welcome onNext={next} />}
      {step === 1 && <Step1Device onNext={goToStep1} onBack={back} />}
      {step === 2 && <Step2Review onNext={next} onBack={back} />}
      {step === 3 && <Step3Install minerKey={minerKey} walletAddress={walletAddress} onDone={onDone} />}
    </div>
  )
}
