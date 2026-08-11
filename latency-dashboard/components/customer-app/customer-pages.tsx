"use client";

import Link from "next/link";
import { useState } from "react";
import { plannedExposure } from "@/lib/customer-config/diff.mjs";
import type { CustomerConfig, TargetWallet } from "@/lib/customer-config/types";
import { useCustomerConfig } from "@/components/customer-app/customer-app-shell";
import styles from "@/components/customer-app/customer-app.module.css";

const DEMO_SECRET = "DEMO_ONLY_NOT_A_REAL_PRIVATE_KEY_HERMES_2026";

function initials(label: string) {
  return label.split(" ").map((word) => word[0]).join("").slice(0, 2).toUpperCase();
}

function PageTop({ status = "Fixture active" }: { status?: string }) {
  return (
    <div className={styles.pageTop}>
      <span className={styles.demoPill}>Demo data</span>
      <div className={styles.pageTopTools}><span className={styles.statusPill}>{status}</span></div>
    </div>
  );
}

function Toggle({ checked, label, onChange }: { checked: boolean; label: string; onChange: (checked: boolean) => void }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      className={`${styles.toggle} ${checked ? styles.toggleOn : ""}`}
      onClick={() => onChange(!checked)}
    />
  );
}

function NumberField({
  id,
  label,
  value,
  unit,
  step = 1,
  min = 0,
  onChange
}: {
  id: string;
  label: string;
  value: number;
  unit: string;
  step?: number;
  min?: number;
  onChange: (value: number) => void;
}) {
  return (
    <div className={styles.field}>
      <label htmlFor={id}>{label}</label>
      <div className={styles.inputShell}>
        <input
          id={id}
          type="number"
          inputMode="decimal"
          min={min}
          step={step}
          value={value}
          onChange={(event) => onChange(Number(event.target.value))}
        />
        <span>{unit}</span>
      </div>
    </div>
  );
}

function AmountField({ config, update }: { config: CustomerConfig; update: (amount: number) => void }) {
  return (
    <div className={styles.field}>
      <label htmlFor="amount-per-buy">Amount per buy</label>
      <div className={styles.inputShell}>
        <input id="amount-per-buy" type="number" inputMode="decimal" min="0" step="0.01" value={config.amountPerBuySol} onChange={(event) => update(Number(event.target.value))} />
        <span>SOL</span>
      </div>
      <div className={styles.presets} aria-label="Amount presets">
        {[0.05, 0.15, 0.25].map((amount) => (
          <button key={amount} type="button" className={`${styles.preset} ${config.amountPerBuySol === amount ? styles.presetActive : ""}`} onClick={() => update(amount)}>{amount.toFixed(2)}</button>
        ))}
      </div>
    </div>
  );
}

function TargetList({ targets, onToggle }: { targets: TargetWallet[]; onToggle: (targetId: string, enabled: boolean) => void }) {
  if (targets.length === 0) {
    return <div className={styles.emptyState}><h3>No target wallets yet</h3><p>Add a wallet to teach Hermes which future buys may trigger your copy-trading plan.</p></div>;
  }
  return (
    <div className={styles.targetList}>
      {targets.map((target) => (
        <div className={styles.targetRow} key={target.id}>
          <i className={styles.targetAvatar}>{initials(target.label)}</i>
          <div><strong>{target.label}</strong><small>{target.address} · {target.copiedTrades} copied trades</small></div>
          <span className={styles.targetAmount}>{target.amountOverrideSol ? `${target.amountOverrideSol.toFixed(2)} SOL override` : "Global amount"}</span>
          <Toggle checked={target.enabled} label={`${target.label} enabled`} onChange={(enabled) => onToggle(target.id, enabled)} />
        </div>
      ))}
    </div>
  );
}

export function OverviewPage() {
  const { draftConfig, setDraftConfig } = useCustomerConfig();
  const activeTargets = draftConfig.targets.filter((target) => target.enabled).length;
  const ready = draftConfig.telegramLinked && draftConfig.tradingWallet?.ready && draftConfig.copyTradingEnabled;

  return (
    <>
      <PageTop status={ready ? "Strategy active" : "Setup required"} />
      <section className={styles.overviewHero}>
        <div>
          <p className={styles.eyebrow}>Mission control / Overview</p>
          <h1 className={styles.overviewTitle}>{ready ? <>Your strategy is <em>in motion.</em></> : <>Your strategy needs <em>attention.</em></>}</h1>
          <p className={styles.pageIntro}>{ready ? `Hermes is watching ${activeTargets} target wallets. Every future copied buy follows your protected plan.` : "Complete the missing account or wallet step before enabling future copied buys."}</p>
        </div>
        <div className={styles.summaryList}>
          <div className={styles.summaryRow}><span>Targets active</span><strong>{activeTargets} / {draftConfig.targets.length}</strong></div>
          <div className={styles.summaryRow}><span>Buy amount</span><strong>{draftConfig.amountPerBuySol.toFixed(2)} SOL</strong></div>
          <div className={styles.summaryRow}><span>Wallet balance</span><strong>{draftConfig.tradingWallet ? `${draftConfig.tradingWallet.balanceSol.toFixed(2)} SOL` : "Missing"}</strong></div>
          <div className={styles.summaryRow}><span>Maximum planned</span><strong>{plannedExposure(draftConfig).toFixed(2)} SOL</strong></div>
          <div className={styles.summaryRow}><span>Protection</span><strong>{draftConfig.stopLossEnabled ? `−${draftConfig.stopLossPercent}%` : "Off"}</strong></div>
        </div>
      </section>
      <section className={styles.section}>
        <div className={styles.sectionHeader}><div><p className={styles.eyebrow}>Copy trading</p><h2>Your buy plan</h2><p>Edits remain local until you review and apply them.</p></div><Link href="/app/copy-trading" className={styles.sectionAction}>Open full settings</Link></div>
        <div className={styles.overviewGrid}>
          <div className={styles.overviewBlock}><div className={styles.overviewBlockHead}><div><strong>Copy trading</strong><span>Future copied buys</span></div><Toggle checked={draftConfig.copyTradingEnabled} label="Copy trading enabled" onChange={(enabled) => setDraftConfig((config) => ({ ...config, copyTradingEnabled: enabled }))} /></div><span className={styles.subtlePill}>{draftConfig.copyTradingEnabled ? "Active" : "Paused"}</span></div>
          <div className={styles.overviewBlock}><div className={styles.overviewBlockHead}><div><strong>Amount per buy</strong><span>Applied to targets without overrides</span></div></div><AmountField config={draftConfig} update={(amountPerBuySol) => setDraftConfig((config) => ({ ...config, amountPerBuySol }))} /></div>
          <div className={styles.overviewBlock}><div className={styles.overviewBlockHead}><div><strong>Target wallets</strong><span>{activeTargets} currently active</span></div><Link href="/app/copy-trading" className={styles.textButton}>Manage</Link></div><TargetList targets={draftConfig.targets.slice(0, 2)} onToggle={(id, enabled) => setDraftConfig((config) => ({ ...config, targets: config.targets.map((target) => target.id === id ? { ...target, enabled } : target) }))} /></div>
          <div className={styles.overviewBlock}><div className={styles.overviewBlockHead}><div><strong>Exit protection</strong><span>Two profit levels and a stop</span></div></div><div className={styles.ladder}><div className={styles.ladderRow}><span>+35%</span><div className={styles.ladderTrack}><i style={{ width: "45%" }} /></div><b>Sell 35%</b></div><div className={styles.ladderRow}><span>+80%</span><div className={styles.ladderTrack}><i style={{ width: "76%" }} /></div><b>Sell 40%</b></div><div className={`${styles.ladderRow} ${styles.ladderDanger}`}><span>−{draftConfig.stopLossPercent}%</span><div className={styles.ladderTrack}><i style={{ width: "30%" }} /></div><b>Protect</b></div></div></div>
        </div>
      </section>
    </>
  );
}

export function CopyTradingPage() {
  const { draftConfig, setDraftConfig } = useCustomerConfig();
  const update = (patch: Partial<CustomerConfig>) => setDraftConfig((config) => ({ ...config, ...patch }));

  return (
    <>
      <PageTop />
      <section className={styles.pageHero}><p className={styles.eyebrow}>Copy trading / Full settings</p><h1 className={styles.pageTitle}>Shape every copied trade.</h1><p className={styles.pageIntro}>Control future buys, target wallets, exits, fees, and dev-wallet protection. Existing positions keep their current instructions.</p></section>
      <section className={styles.section}>
        <div className={styles.sectionHeader}><div><h2>Buy controls</h2><p>The global defaults used when a target has no override.</p></div></div>
        <div className={styles.settingRows}>
          <div className={styles.settingRow}><div className={styles.settingCopy}><strong>Copy trading</strong><span>Pause or resume future copied buys.</span></div><div className={styles.settingControl}><Toggle checked={draftConfig.copyTradingEnabled} label="Copy trading enabled" onChange={(copyTradingEnabled) => update({ copyTradingEnabled })} /></div></div>
          <div className={styles.settingRow}><div className={styles.settingCopy}><strong>Amount per buy</strong><span>Maximum SOL before fees and slippage.</span></div><div className={styles.settingControl}><AmountField config={draftConfig} update={(amountPerBuySol) => update({ amountPerBuySol })} /></div></div>
          <div className={styles.settingRow}><div className={styles.settingCopy}><strong>Daily buy limit</strong><span>Stop opening new copied positions after this count.</span></div><div className={styles.settingControl}><NumberField id="daily-buys" label="Maximum daily buys" value={draftConfig.maxDailyBuys} unit="buys" onChange={(maxDailyBuys) => update({ maxDailyBuys })} /></div></div>
          <div className={styles.settingRow}><div className={styles.settingCopy}><strong>Position limit</strong><span>Maximum SOL assigned to one copied position.</span></div><div className={styles.settingControl}><NumberField id="position-limit" label="Maximum position" value={draftConfig.maxPositionSol} unit="SOL" step={0.05} onChange={(maxPositionSol) => update({ maxPositionSol })} /></div></div>
        </div>
      </section>
      <section className={styles.section}>
        <div className={styles.sectionHeader}><div><h2>Target wallets</h2><p>Each enabled wallet can trigger future copied buys.</p></div><button type="button" className={styles.sectionAction} onClick={() => setDraftConfig((config) => ({ ...config, targets: [...config.targets, { id: `demo-${config.targets.length}`, label: "New demo wallet", address: "8fADEMO9zQ", enabled: false, copiedTrades: 0, amountOverrideSol: null }] }))}>Add demo wallet</button></div>
        <TargetList targets={draftConfig.targets} onToggle={(id, enabled) => setDraftConfig((config) => ({ ...config, targets: config.targets.map((target) => target.id === id ? { ...target, enabled } : target) }))} />
      </section>
      <section className={styles.section}>
        <div className={styles.sectionHeader}><div><h2>Exit strategy</h2><p>Stage profit-taking while preserving a protected runner.</p></div></div>
        <div className={styles.strategyGrid}>
          <div className={styles.strategyPanel}><h3>Profit ladder</h3><p>Sell 35% at +35%, then 40% at +80%.</p><div className={styles.ladder}>{draftConfig.exitLevels.map((level, index) => <div className={styles.ladderRow} key={level.id}><span>+{level.triggerPercent}%</span><div className={styles.ladderTrack}><i style={{ width: `${Math.min(level.triggerPercent, 100)}%` }} /></div><b>Sell {level.sellPercent}%</b></div>)}</div></div>
          <div className={styles.strategyPanel}><h3>Downside protection</h3><p>A stop and trailing exit protect what remains.</p><div className={styles.settingRows}><div className={styles.settingRow}><div className={styles.settingCopy}><strong>Stop loss</strong><span>Below entry</span></div><div className={styles.settingControl}><Toggle checked={draftConfig.stopLossEnabled} label="Stop loss enabled" onChange={(stopLossEnabled) => update({ stopLossEnabled })} /></div></div><div className={styles.settingRow}><div className={styles.settingCopy}><strong>Stop trigger</strong><span>Percent below entry</span></div><div className={styles.settingControl}><NumberField id="stop-loss" label="Stop loss" value={draftConfig.stopLossPercent} unit="%" onChange={(stopLossPercent) => update({ stopLossPercent })} /></div></div><div className={styles.settingRow}><div className={styles.settingCopy}><strong>Trailing stop</strong><span>Follow favorable movement</span></div><div className={styles.settingControl}><Toggle checked={draftConfig.trailingStopEnabled} label="Trailing stop enabled" onChange={(trailingStopEnabled) => update({ trailingStopEnabled })} /></div></div></div></div>
        </div>
      </section>
      <section className={styles.section}>
        <div className={styles.sectionHeader}><div><h2>Risk and fees</h2><p>Explicit inputs keep execution limits understandable.</p></div></div>
        <div className={styles.settingRows}>
          <div className={styles.settingRow}><div className={styles.settingCopy}><strong>Buy slippage</strong><span>Maximum accepted range for buys.</span></div><div className={styles.settingControl}><NumberField id="buy-slippage" label="Buy slippage" value={draftConfig.buySlippagePercent} unit="%" step={0.5} onChange={(buySlippagePercent) => update({ buySlippagePercent })} /></div></div>
          <div className={styles.settingRow}><div className={styles.settingCopy}><strong>Sell slippage</strong><span>Maximum accepted range for exits.</span></div><div className={styles.settingControl}><NumberField id="sell-slippage" label="Sell slippage" value={draftConfig.sellSlippagePercent} unit="%" step={0.5} onChange={(sellSlippagePercent) => update({ sellSlippagePercent })} /></div></div>
          <div className={styles.settingRow}><div className={styles.settingCopy}><strong>Priority fee</strong><span>Maximum additional fee per transaction.</span></div><div className={styles.settingControl}><NumberField id="priority-fee" label="Priority fee" value={draftConfig.priorityFeeSol} unit="SOL" step={0.0005} onChange={(priorityFeeSol) => update({ priorityFeeSol })} /></div></div>
        </div>
      </section>
      <section className={styles.section}>
        <div className={styles.sectionHeader}><div><h2>Dev-wallet protection</h2><p>Extra controls for creator-related activity.</p></div></div>
        <div className={styles.settingRows}>
          <div className={styles.settingRow}><div className={styles.settingCopy}><strong>Dev sniping</strong><span>Allow supported creator-launch rules.</span></div><div className={styles.settingControl}><Toggle checked={draftConfig.devSnipingEnabled} label="Dev sniping enabled" onChange={(devSnipingEnabled) => update({ devSnipingEnabled })} /></div></div>
          <div className={styles.settingRow}><div className={styles.settingCopy}><strong>Block while dev sells</strong><span>Prevent new buys while creator selling is detected.</span></div><div className={styles.settingControl}><Toggle checked={draftConfig.blockDevSelling} label="Block while dev sells" onChange={(blockDevSelling) => update({ blockDevSelling })} /></div></div>
        </div>
      </section>
    </>
  );
}

export function WalletsPage() {
  const { draftConfig, setDraftConfig } = useCustomerConfig();
  const [creating, setCreating] = useState(false);
  const [confirmed, setConfirmed] = useState(false);
  const [revealed, setRevealed] = useState(false);
  const [hiddenPermanently, setHiddenPermanently] = useState(false);
  const [copied, setCopied] = useState(false);

  async function copyDemoSecret() {
    try {
      await navigator.clipboard.writeText(DEMO_SECRET);
      setCopied(true);
    } catch {
      setCopied(false);
    }
  }

  function useDemoWallet() {
    setDraftConfig((config) => ({ ...config, tradingWallet: { address: "7xHDEMOwallet9Q", balanceSol: 0, ready: true } }));
    setCreating(false);
    setRevealed(false);
    setConfirmed(false);
    setHiddenPermanently(false);
  }

  function startDemoWalletFlow() {
    setCreating(true);
    setConfirmed(false);
    setRevealed(false);
    setHiddenPermanently(false);
    setCopied(false);
  }

  function hideDemoSecret() {
    setRevealed(false);
    setHiddenPermanently(true);
  }

  return (
    <>
      <PageTop status={draftConfig.tradingWallet ? "Wallet ready" : "Wallet required"} />
      <section className={styles.pageHero}><p className={styles.eyebrow}>Wallet custody / Demo only</p><h1 className={styles.pageTitle}>Your trading wallet.</h1><p className={styles.pageIntro}>Fund, inspect, or create a fictional wallet flow. No real key material is generated, stored, or transmitted.</p></section>
      <section className={styles.section}>
        {draftConfig.tradingWallet ? (
          <div className={styles.walletGrid}>
            <div className={styles.walletPrimary}><div><span className={styles.walletLabel}>Available balance</span><div className={styles.walletBalance}>{draftConfig.tradingWallet.balanceSol.toFixed(2)} SOL</div><div className={styles.walletAddress}>{draftConfig.tradingWallet.address}</div></div><div className={styles.walletActions}><button type="button">Copy deposit address</button><button type="button" onClick={startDemoWalletFlow}>Create another demo wallet</button></div></div>
            <div className={styles.walletPanel}><h3>Wallet readiness</h3><p>This fictional wallet is associated with the customer fixture and can be selected by the copy-trading plan.</p><div className={styles.summaryList}><div className={styles.summaryRow}><span>Custody state</span><strong>Demo ready</strong></div><div className={styles.summaryRow}><span>Signing</span><strong>Never performed</strong></div><div className={styles.summaryRow}><span>Network calls</span><strong>None</strong></div></div></div>
          </div>
        ) : <div className={styles.emptyState}><h3>No trading wallet</h3><p>Create a fictional wallet to exercise the frontend flow. Copy trading remains paused.</p><button type="button" className={styles.sectionAction} onClick={startDemoWalletFlow}>Create demo wallet</button></div>}
      </section>
      {creating ? (
        <section className={styles.section}>
          <div className={styles.walletPanel}><h3>One-time demo secret</h3><p>The value below is a fixed label, not a valid Solana private key. Hiding it simulates the one-time reveal boundary.</p>
            <div className={styles.confirmationRow}><input id="secret-confirm" type="checkbox" checked={confirmed} onChange={(event) => setConfirmed(event.target.checked)} /><label htmlFor="secret-confirm">I understand this is fictional and that a real secret must never be shared or stored in browser history.</label></div>
            {!revealed && !hiddenPermanently ? <button type="button" className={styles.sectionAction} disabled={!confirmed} onClick={() => setRevealed(true)}>Reveal fictional secret</button> : null}
            {revealed ? <div className={styles.secretBox}><strong>DEMO VALUE — CANNOT CONTROL FUNDS</strong><code>{DEMO_SECRET}</code><p>Once hidden, this prototype will not reveal it again until the flow is restarted.</p></div> : null}
            {hiddenPermanently ? <div className={styles.secretBox}><strong>Demo secret hidden</strong><p>This creation flow cannot reveal it again. Start a new demo flow to generate another fictional wallet.</p></div> : null}
            {revealed ? <div className={styles.walletActions}><button type="button" onClick={copyDemoSecret}>{copied ? "Copied demo text" : "Copy demo text"}</button><button type="button" onClick={hideDemoSecret}>Hide permanently</button><button type="button" onClick={useDemoWallet}>Use demo wallet</button></div> : null}
            {hiddenPermanently ? <div className={styles.walletActions}><button type="button" onClick={useDemoWallet}>Continue with demo wallet</button><button type="button" onClick={startDemoWalletFlow}>Restart demo flow</button></div> : null}
          </div>
        </section>
      ) : null}
    </>
  );
}

export function AlertsPage() {
  const { draftConfig, setDraftConfig } = useCustomerConfig();
  const labels: Array<[keyof CustomerConfig["alerts"], string, string]> = [
    ["copiedBuy", "Copied buys", "Notify when a copied buy is accepted."],
    ["copiedSell", "Copied sells", "Notify when an exit is submitted."],
    ["positionWarning", "Position warnings", "Notify when protection or balance needs attention."],
    ["runtimeFailure", "Runtime failures", "Notify when a revision or execution cannot become active."],
    ["dailySummary", "Daily summary", "Send one combined activity summary each day."]
  ];
  return (
    <><PageTop /><section className={styles.pageHero}><p className={styles.eyebrow}>Notifications</p><h1 className={styles.pageTitle}>Stay informed, not interrupted.</h1><p className={styles.pageIntro}>Choose the Telegram alerts that deserve immediate attention. Every option is part of the same staged review.</p></section><section className={styles.section}><div className={styles.settingRows}>{labels.map(([key, label, description]) => <div className={styles.settingRow} key={key}><div className={styles.settingCopy}><strong>{label}</strong><span>{description}</span></div><div className={styles.settingControl}><Toggle checked={draftConfig.alerts[key]} label={`${label} enabled`} onChange={(enabled) => setDraftConfig((config) => ({ ...config, alerts: { ...config.alerts, [key]: enabled } }))} /></div></div>)}</div></section></>
  );
}

export function ActivityPage() {
  const { activity } = useCustomerConfig();
  return (
    <><PageTop /><section className={styles.pageHero}><p className={styles.eyebrow}>Changes and trades</p><h1 className={styles.pageTitle}>Activity with context.</h1><p className={styles.pageIntro}>Configuration revisions and personal copy-trade outcomes share one timeline so cause and effect stay visible.</p></section><section className={styles.section}><div className={styles.activityList}>{activity.map((item) => <div className={styles.activityRow} key={item.id}><time>{item.occurredAt}</time><div><strong>{item.title}</strong><p>{item.detail}</p></div><span className={styles.activityStatus}>{item.status}</span></div>)}</div></section></>
  );
}

export function CashbackPage() {
  const { draftConfig } = useCustomerConfig();
  return (
    <><PageTop /><section className={styles.pageHero}><p className={styles.eyebrow}>Cashback and referrals</p><h1 className={styles.pageTitle}>Share Hermes. Earn together.</h1><p className={styles.pageIntro}>Fixture-only referral and cashback information for the complete customer journey.</p></section><section className={styles.section}><div className={styles.cashbackHero}><div><span className={styles.walletLabel}>Total demo cashback</span><div className={styles.cashbackAmount}>{draftConfig.cashback.earnedSol.toFixed(3)} SOL</div></div><span className={styles.addressPill}>{draftConfig.cashback.referralCode}</span></div><div className={styles.cashbackStats}><div className={styles.cashbackStat}><span>Invited users</span><strong>{draftConfig.cashback.invitedUsers}</strong></div><div className={styles.cashbackStat}><span>Pending payout</span><strong>0.042 SOL</strong></div></div></section></>
  );
}

export function AccountPage() {
  const { draftConfig, setDraftConfig } = useCustomerConfig();
  return (
    <><PageTop status={draftConfig.telegramLinked ? "Telegram linked" : "Link required"} /><section className={styles.pageHero}><p className={styles.eyebrow}>Identity and account</p><h1 className={styles.pageTitle}>Your Hermes connection.</h1><p className={styles.pageIntro}>This simulated link establishes which Telegram identity and customer configuration the frontend represents.</p></section><section className={styles.section}><div className={styles.accountGrid}><div className={styles.accountPanel}><h3>Telegram account</h3><p>{draftConfig.telegramLinked ? `${draftConfig.telegramHandle} is linked to this demo session.` : "No Telegram identity is linked."}</p><button type="button" className={styles.sectionAction} onClick={() => setDraftConfig((config) => ({ ...config, telegramLinked: !config.telegramLinked, telegramHandle: config.telegramLinked ? "Not linked" : "@xloupee" }))}>{draftConfig.telegramLinked ? "Unlink demo account" : "Link demo account"}</button></div><div className={styles.accountPanel}><h3>Prototype boundaries</h3><p>Everything under `/app` is fixture data. It does not authenticate, send Telegram messages, call Supabase, sign transactions, or contact a trading runtime.</p><Link href="/app/welcome" className={styles.sectionAction}>Replay onboarding</Link></div></div></section></>
  );
}

export function WelcomePage() {
  return (
    <div className={styles.welcomeCard}>
      <section className={styles.welcomeStory}><div className={styles.brand}><span className={styles.brandMark}>H</span><div><strong>Hermes</strong><small>Customer configuration</small></div></div><h1>Your bot, made <em>visible.</em></h1><p>Move from Telegram commands to a calm desktop workspace while keeping deliberate review before every change.</p></section>
      <section className={styles.welcomeSteps}><p className={styles.eyebrow}>Simulated onboarding</p><h2>Connect your account</h2><p>No identity is verified in this prototype. These steps only demonstrate the intended journey.</p><div className={styles.welcomeStep}><i>01</i><div><strong>Sign in to Hermes</strong><span>Start a fictional browser session.</span></div></div><div className={styles.welcomeStep}><i>02</i><div><strong>Link Telegram</strong><span>Associate the immutable Telegram identity that will own settings later.</span></div></div><div className={styles.welcomeStep}><i>03</i><div><strong>Review your active plan</strong><span>Enter Mission Control with fixture settings.</span></div></div><Link className={styles.welcomeLink} href="/app">Enter the demo workspace</Link></section>
    </div>
  );
}
