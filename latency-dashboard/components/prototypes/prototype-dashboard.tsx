"use client";

import Link from "next/link";
import {
  ArrowLeft,
  ArrowRight,
  Check,
  ChevronRight,
  CircleAlert,
  Clock3,
  Copy,
  Ellipsis,
  Filter,
  Pause,
  Play,
  RefreshCw,
  Search,
  SlidersHorizontal
} from "lucide-react";
import { useState } from "react";
import {
  PROTOTYPE_DIRECTIONS,
  type PrototypeDirection,
  type PrototypeExecution,
  type PrototypePreset,
  filterPrototypeExecutions
} from "./prototype-data";
import styles from "./prototypes.module.css";

const PRESETS: { id: PrototypePreset; label: string }[] = [
  { id: "all", label: "All executions" },
  { id: "landed-buys", label: "Landed buys" },
  { id: "landed-sells", label: "Landed sells" },
  { id: "issues", label: "Needs review" }
];

function outcomeLabel(row: PrototypeExecution) {
  if (row.outcome === "landed") return row.landing;
  return row.outcome === "failed" ? "Failed on-chain" : "Skipped";
}

function StatusMark({ outcome }: { outcome: PrototypeExecution["outcome"] }) {
  return <span className={`${styles.statusMark} ${styles[`status_${outcome}`]}`} aria-hidden="true" />;
}

function CopyValue({ value, short }: { value: string; short: string }) {
  const [copyState, setCopyState] = useState<"idle" | "copied" | "failed">("idle");

  async function copy() {
    try {
      await navigator.clipboard.writeText(value);
      setCopyState("copied");
    } catch {
      setCopyState("failed");
    }
    window.setTimeout(() => setCopyState("idle"), 1500);
  }

  return (
    <button className={styles.copyValue} onClick={copy} type="button" title={`Copy ${value}`}>
      <span>{copyState === "copied" ? "Copied" : copyState === "failed" ? "Copy failed" : short}</span>
      {copyState === "copied" ? <Check size={13} /> : <Copy size={13} />}
    </button>
  );
}

function Presets({ active, onChange, compact = false }: {
  active: PrototypePreset;
  onChange: (value: PrototypePreset) => void;
  compact?: boolean;
}) {
  return (
    <div className={`${styles.presets} ${compact ? styles.presetsCompact : ""}`} aria-label="Execution presets">
      {PRESETS.map((preset) => (
        <button
          type="button"
          key={preset.id}
          className={active === preset.id ? styles.presetActive : ""}
          aria-pressed={active === preset.id}
          onClick={() => onChange(preset.id)}
        >
          {preset.label}
        </button>
      ))}
    </div>
  );
}

function UtilityControls({ paused, onToggle }: { paused: boolean; onToggle: () => void }) {
  return (
    <div className={styles.utilityControls}>
      <span className={styles.liveState}><i /> Synced 12s ago</span>
      <button type="button" onClick={onToggle} aria-label={paused ? "Resume automatic refresh" : "Pause automatic refresh"}>
        {paused ? <Play size={15} /> : <Pause size={15} />}
        {paused ? "Resume" : "Pause"}
      </button>
      <button type="button"><RefreshCw size={15} /> Refresh</button>
    </div>
  );
}

function Metrics({ treatment = "default" }: { treatment?: string }) {
  const metrics = [
    ["Landed buys", "29", "+4 today"],
    ["Landed sells", "104", "+17 today"],
    ["Landing rate", "95.7%", "+1.8% vs 7d"],
    ["Non-landed", "6", "2 need review"]
  ];
  return (
    <div className={`${styles.metrics} ${styles[`metrics_${treatment}`]}`}>
      {metrics.map(([label, value, note]) => (
        <div className={styles.metricItem} key={label}>
          <span>{label}</span>
          <strong>{value}</strong>
          <small>{note}</small>
        </div>
      ))}
    </div>
  );
}

function CleanTable({ rows, numbered = false, showLatency = true }: {
  rows: PrototypeExecution[];
  numbered?: boolean;
  showLatency?: boolean;
}) {
  if (rows.length === 0) return <p className={styles.prototypeEmpty}>No executions in this view.</p>;
  return (
    <div className={styles.cleanTableWrap}>
      <table className={styles.cleanTable}>
        <thead>
          <tr>
            {numbered ? <th aria-label="Row number" /> : null}
            <th>Time</th>
            <th>Side</th>
            <th>Token</th>
            <th>Watched wallet</th>
            <th>Outcome</th>
            <th>Route</th>
            {showLatency ? <th>Observed → ack</th> : null}
            <th aria-label="Open execution" />
          </tr>
        </thead>
        <tbody>
          {rows.map((row, index) => (
            <tr key={row.id}>
              {numbered ? <td className={styles.rowNumber}>{String(index + 1).padStart(2, "0")}</td> : null}
              <td><strong>{row.time}</strong><small>{row.day}</small></td>
              <td><span className={`${styles.sideLabel} ${styles[`side_${row.side}`]}`}>{row.side}</span></td>
              <td><strong>{row.token}</strong><small>{row.mint.slice(0, 7)}…pump</small></td>
              <td><CopyValue value={row.wallet} short={row.walletShort} /></td>
              <td>
                <span className={styles.outcomeLine}><StatusMark outcome={row.outcome} />{outcomeLabel(row)}</span>
                <small>{row.landingDetail}</small>
              </td>
              <td><strong>{row.provider}</strong><small>{row.route}</small></td>
              {showLatency ? <td>{row.latency}</td> : null}
              <td><button className={styles.rowOpen} aria-label={`Open ${row.id}`} type="button"><ChevronRight size={16} /></button></td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function MobileRows({ rows }: { rows: PrototypeExecution[] }) {
  return (
    <div className={styles.mobileRows}>
      {rows.map((row) => (
        <article key={row.id}>
          <div><StatusMark outcome={row.outcome} /><span>{row.time}</span><span className={`${styles.sideLabel} ${styles[`side_${row.side}`]}`}>{row.side}</span></div>
          <h3>{row.token} <small>via {row.provider}</small></h3>
          <p>{outcomeLabel(row)} · {row.landingDetail}</p>
          <CopyValue value={row.wallet} short={row.walletShort} />
        </article>
      ))}
    </div>
  );
}

function Ledger({ rows, preset, setPreset, paused, togglePause }: PrototypeProps) {
  return (
    <div className={`${styles.direction} ${styles.ledger}`}>
      <aside className={styles.ledgerRail}>
        <div className={styles.wordmark}>H<span>®</span></div>
        <nav><span className={styles.navSelected}>Overview</span><span>Executions</span><span>Sources</span><span>System</span></nav>
        <div className={styles.railFoot}><span>Operator 01</span><i /></div>
      </aside>
      <main className={styles.ledgerMain}>
        <header className={styles.ledgerHeader}>
          <div><p>Friday · 31 July</p><h1>Execution ledger</h1></div>
          <UtilityControls paused={paused} onToggle={togglePause} />
        </header>
        <Metrics treatment="ledger" />
        <div className={styles.ledgerSectionHead}>
          <div><h2>Recent activity</h2><p>Ordered by observed time</p></div>
          <Presets active={preset} onChange={setPreset} />
        </div>
        <CleanTable rows={rows} />
        <MobileRows rows={rows} />
      </main>
    </div>
  );
}

function Briefing({ rows, preset, setPreset, paused, togglePause }: PrototypeProps) {
  const lead = rows[0];
  return (
    <div className={`${styles.direction} ${styles.briefing}`}>
      <header className={styles.briefMasthead}>
        <div className={styles.briefBrand}>Hermes <span>Operator Briefing</span></div>
        <div>Issue № 214<br /><strong>31 July 2026</strong></div>
      </header>
      <nav className={styles.briefNav}><span className={styles.navSelected}>Overview</span><span>Executions</span><span>Sources</span><span>System</span><UtilityControls paused={paused} onToggle={togglePause} /></nav>
      <main className={styles.briefMain}>
        <div className={styles.briefHeadline}>
          <p className={styles.sectionKicker}>State of execution</p>
          <h1>The desk is landing<br />ninety-five in a hundred.</h1>
          <p>133 confirmed copies in the current window. Sell coverage is strong; two failed buys account for the only actionable exceptions.</p>
        </div>
        <div className={styles.briefScore}><span>Landing rate</span><strong>95.7<sup>%</sup></strong><small>+1.8 points versus seven-day baseline</small></div>
        <section className={styles.briefColumns}>
          <article>
            <span className={styles.storyNumber}>01</span>
            <p className={styles.sectionKicker}>What changed</p>
            <h2>Sell execution is carrying the window.</h2>
            <p>104 landed sells, including 18 without a target comparison. Those rows are counted as landed and described by their confirmed copy slot.</p>
          </article>
          <article>
            <span className={styles.storyNumber}>02</span>
            <p className={styles.sectionKicker}>Needs attention</p>
            <h2>Two failures share the direct TPU route.</h2>
            <p>Both failed on-chain after acknowledgment. Provider health is current; no source freshness warning is active.</p>
          </article>
          <article className={styles.latestStory}>
            <p className={styles.sectionKicker}>Latest execution</p>
            {lead ? <><h2>{lead.side.toUpperCase()} {lead.token} landed.</h2><p>{lead.landing} · {lead.landingDetail}. Routed through {lead.provider} at {lead.time}.</p><CopyValue value={lead.wallet} short={lead.walletShort} /></> : <p>No execution in this preset.</p>}
          </article>
        </section>
        <div className={styles.briefActivityHead}><h2>The activity record</h2><Presets active={preset} onChange={setPreset} /></div>
        <CleanTable rows={rows} showLatency={false} />
        <MobileRows rows={rows} />
      </main>
    </div>
  );
}

function Tape({ rows, preset, setPreset, paused, togglePause }: PrototypeProps) {
  return (
    <div className={`${styles.direction} ${styles.tape}`}>
      <header className={styles.tapeHeader}>
        <div className={styles.tapeLogo}>HERMES<span>/OPS</span></div>
        <nav><span className={styles.navSelected}>Tape</span><span>Executions</span><span>Sources</span><span>System</span></nav>
        <UtilityControls paused={paused} onToggle={togglePause} />
      </header>
      <div className={styles.ticker}>
        <span>LANDED BUY <strong>29</strong> <i>+4</i></span>
        <span>LANDED SELL <strong>104</strong> <i>+17</i></span>
        <span>LANDING RATE <strong>95.7%</strong> <i>+1.8</i></span>
        <span>NON-LANDED <strong>6</strong> <i className={styles.tickerWarn}>2 REVIEW</i></span>
        <span>SOURCE AGE <strong>12s</strong> <i>HEALTHY</i></span>
      </div>
      <main className={styles.tapeMain}>
        <div className={styles.tapeToolbar}>
          <div><span>LIVE EXECUTION TAPE</span><small>{rows.length} OF 9,288 RECORDS</small></div>
          <Presets compact active={preset} onChange={setPreset} />
        </div>
        <div className={styles.tapeTable} role="table" aria-label="Live execution tape">
          <div className={styles.tapeTableHead} role="row"><span>TIME</span><span>ACT</span><span>ASSET</span><span>WALLET</span><span>RESULT / PLACEMENT</span><span>ROUTE</span><span>ACK</span><span>TX</span></div>
          {rows.map((row) => (
            <div className={styles.tapeRow} role="row" key={row.id}>
              <span>{row.time}.<small>412</small></span>
              <span className={styles[`tape_${row.side}`]}>{row.side.toUpperCase()}</span>
              <span><strong>{row.token}</strong><small>{row.mint.slice(0, 5)}…</small></span>
              <CopyValue value={row.wallet} short={row.walletShort} />
              <span className={styles[`tape_${row.outcome}`]}><StatusMark outcome={row.outcome} />{outcomeLabel(row)}<small>{row.landingDetail}</small></span>
              <span>{row.provider}<small>{row.route}</small></span>
              <span>{row.latency}</span>
              <CopyValue value={row.signature} short={row.signatureShort} />
            </div>
          ))}
        </div>
        <MobileRows rows={rows} />
      </main>
    </div>
  );
}

function IndexDirection({ rows, preset, setPreset, paused, togglePause }: PrototypeProps) {
  return (
    <div className={`${styles.direction} ${styles.indexDirection}`}>
      <header className={styles.indexHeader}>
        <div className={styles.indexLogo}>HERMES®</div>
        <nav><span className={styles.navSelected}>01 Overview</span><span>02 Executions</span><span>03 Sources</span><span>04 System</span></nav>
        <UtilityControls paused={paused} onToggle={togglePause} />
      </header>
      <main className={styles.indexMain}>
        <aside><span>01</span><p>Overview<br />31—07—26</p></aside>
        <div className={styles.indexContent}>
          <div className={styles.indexTitle}><p>Solana execution intelligence</p><h1>Current<br />window</h1></div>
          <Metrics treatment="index" />
          <section className={styles.indexRecord}>
            <div className={styles.indexRecordHead}>
              <div><span>01—A</span><h2>Execution record</h2></div>
              <Presets active={preset} onChange={setPreset} />
            </div>
            <CleanTable rows={rows} numbered showLatency={false} />
            <MobileRows rows={rows} />
          </section>
        </div>
      </main>
    </div>
  );
}

function Concierge({ rows, preset, setPreset, paused, togglePause }: PrototypeProps) {
  return (
    <div className={`${styles.direction} ${styles.concierge}`}>
      <aside className={styles.conciergeRail}>
        <div><span>H</span><p>Hermes<br /><small>Operator desk</small></p></div>
        <nav><span className={styles.navSelected}>Today</span><span>Executions</span><span>Sources</span><span>System</span></nav>
        <div className={styles.conciergeOperator}><i /> Kenneth<br /><small>Administrator</small></div>
      </aside>
      <main className={styles.conciergeMain}>
        <header><div><p>Good afternoon</p><h1>Here’s what needs<br />your attention.</h1></div><UtilityControls paused={paused} onToggle={togglePause} /></header>
        <section className={styles.attentionBand}>
          <CircleAlert size={24} />
          <div><strong>2 failed buys share the direct TPU route.</strong><span>Last failure 18 minutes ago · other providers are healthy</span></div>
          <button type="button">Review executions <ArrowRight size={16} /></button>
        </section>
        <div className={styles.conciergeSplit}>
          <section>
            <p className={styles.sectionKicker}>Desk health</p>
            <strong className={styles.conciergeRate}>95.7<sup>%</sup></strong>
            <p>Landing rate is 1.8 points above the seven-day baseline.</p>
            <div className={styles.conciergeMiniMetrics}><span><b>29</b>Landed buys</span><span><b>104</b>Landed sells</span><span><b>6</b>Non-landed</span></div>
          </section>
          <section>
            <p className={styles.sectionKicker}>System freshness</p>
            <ul><li><i />Execution ingestion <span>12 sec</span></li><li><i />Source observations <span>8 sec</span></li><li><i />Database query <span>84 ms</span></li></ul>
          </section>
        </div>
        <div className={styles.conciergeActivityHead}><div><p className={styles.sectionKicker}>Latest activity</p><h2>Execution stream</h2></div><Presets active={preset} onChange={setPreset} /></div>
        <CleanTable rows={rows} showLatency={false} />
        <MobileRows rows={rows} />
      </main>
    </div>
  );
}

function Timeline({ rows, preset, setPreset, paused, togglePause }: PrototypeProps) {
  return (
    <div className={`${styles.direction} ${styles.timeline}`}>
      <header className={styles.timelineHeader}><div className={styles.timelineBrand}>Hermes <span>Chronicle</span></div><nav><span className={styles.navSelected}>Today</span><span>Archive</span><span>Sources</span></nav><UtilityControls paused={paused} onToggle={togglePause} /></header>
      <main className={styles.timelineMain}>
        <section className={styles.timelineIntro}><p className={styles.sectionKicker}>Friday · 31 July</p><h1>A day in<br />execution.</h1><p>Read each copied trade in the order it happened, with placement and exceptions kept in context.</p><Presets active={preset} onChange={setPreset} /></section>
        <section className={styles.timelineStream}>
          <div className={styles.timelineStreamHead}><span>Pacific time</span><span>{rows.length} events in view</span></div>
          {rows.map((row) => (
            <article key={row.id} className={styles.timelineEvent}>
              <time>{row.time}</time>
              <div className={styles.timelineDot}><StatusMark outcome={row.outcome} /></div>
              <div className={styles.timelineEventBody}>
                <p><span className={`${styles.sideLabel} ${styles[`side_${row.side}`]}`}>{row.side}</span><strong>{row.token}</strong><small>via {row.provider}</small></p>
                <h2>{outcomeLabel(row)}</h2>
                <p>{row.landingDetail}. Watched <CopyValue value={row.wallet} short={row.walletShort} /> and returned acknowledgment in {row.latency}.</p>
              </div>
              <button type="button" aria-label={`Open ${row.id}`}><ArrowRight size={17} /></button>
            </article>
          ))}
        </section>
        <aside className={styles.timelineAside}>
          <span>Current window</span><strong>133</strong><p>landed copies</p>
          <dl><div><dt>Buy</dt><dd>29</dd></div><div><dt>Sell</dt><dd>104</dd></div><div><dt>Rate</dt><dd>95.7%</dd></div></dl>
        </aside>
      </main>
    </div>
  );
}

function Command({ rows, preset, setPreset, paused, togglePause }: PrototypeProps) {
  return (
    <div className={`${styles.direction} ${styles.command}`}>
      <header className={styles.commandHeader}><div className={styles.commandBrand}>HERMES <span>COMMAND</span></div><nav><span className={styles.navSelected}>Overview</span><span>Sources</span><span>System</span></nav><UtilityControls paused={paused} onToggle={togglePause} /></header>
      <main className={styles.commandMain}>
        <div className={styles.commandTitle}><div><p>Execution workspace / current window</p><h1>9,288 executions</h1></div><Metrics treatment="command" /></div>
        <section className={styles.commandBar}>
          <button type="button"><Search size={16} /> Search wallet, mint, signature <kbd>⌘ K</kbd></button>
          <button type="button"><Clock3 size={16} /> Last 24 hours <ChevronRight size={15} /></button>
          <button type="button"><SlidersHorizontal size={16} /> More filters <span>3</span></button>
        </section>
        <div className={styles.commandPresets}><Presets active={preset} onChange={setPreset} /><div><button type="button"><Filter size={15} /> Save view</button><button type="button"><Ellipsis size={17} aria-label="More options" /></button></div></div>
        <div className={styles.commandResultMeta}><span>{rows.length} sample records · sorted newest first</span><span>Columns <b>8</b></span></div>
        <CleanTable rows={rows} numbered />
        <MobileRows rows={rows} />
      </main>
    </div>
  );
}

function Atelier({ rows, preset, setPreset, paused, togglePause }: PrototypeProps) {
  const lead = rows[0];
  return (
    <div className={`${styles.direction} ${styles.atelier}`}>
      <header className={styles.atelierHeader}><div className={styles.atelierBrand}>Hermes Trader</div><nav><span className={styles.navSelected}>Desk</span><span>Executions</span><span>Sources</span><span>System</span></nav><UtilityControls paused={paused} onToggle={togglePause} /></header>
      <main className={styles.atelierMain}>
        <section className={styles.atelierHero}>
          <div className={styles.atelierTitle}><p>Operator intelligence · 31 July</p><h1>The desk,<br /><em>in motion.</em></h1></div>
          <div className={styles.atelierRate}><span>Landing rate</span><strong>95.7<sup>%</sup></strong><p>133 landed copies<br />6 non-landed attempts</p></div>
          <div className={styles.atelierNote}><span>01 / Overview</span><p>Execution is healthy. The current window is outperforming its seven-day baseline by 1.8 percentage points.</p></div>
        </section>
        <section className={styles.atelierFeature}>
          <div><p className={styles.sectionKicker}>Latest movement</p>{lead ? <><h2>{lead.side === "sell" ? "A sell" : "A buy"} landed one slot after its target.</h2><p>{lead.token} routed through {lead.provider} at {lead.time}. The copy is confirmed at {lead.landingDetail}.</p><CopyValue value={lead.signature} short={lead.signatureShort} /></> : <h2>No executions in this view.</h2>}</div>
          <div className={styles.atelierCounts}><span><small>Landed buys</small><b>29</b></span><span><small>Landed sells</small><b>104</b></span></div>
        </section>
        <div className={styles.atelierRecordHead}><div><span>02 / Record</span><h2>Recent executions</h2></div><Presets active={preset} onChange={setPreset} /></div>
        <div className={styles.atelierRows}>
          {rows.map((row, index) => (
            <article key={row.id}>
              <span>{String(index + 1).padStart(2, "0")}</span>
              <div><p>{row.time} · {row.provider}</p><h3>{row.side.toUpperCase()} {row.token}</h3></div>
              <div><p><StatusMark outcome={row.outcome} /> {outcomeLabel(row)}</p><small>{row.landingDetail}</small></div>
              <CopyValue value={row.wallet} short={row.walletShort} />
              <button type="button" aria-label={`Open ${row.id}`}><ArrowRight size={17} /></button>
            </article>
          ))}
        </div>
      </main>
    </div>
  );
}

interface PrototypeProps {
  rows: PrototypeExecution[];
  preset: PrototypePreset;
  setPreset: (value: PrototypePreset) => void;
  paused: boolean;
  togglePause: () => void;
}

const RENDERERS: Record<PrototypeDirection["slug"], (props: PrototypeProps) => React.ReactNode> = {
  ledger: Ledger,
  briefing: Briefing,
  tape: Tape,
  index: IndexDirection,
  concierge: Concierge,
  timeline: Timeline,
  command: Command,
  atelier: Atelier
};

export function PrototypeDashboard({ direction }: { direction: PrototypeDirection }) {
  const [preset, setPreset] = useState<PrototypePreset>("all");
  const [paused, setPaused] = useState(false);
  const rows = filterPrototypeExecutions(preset);
  const currentIndex = PROTOTYPE_DIRECTIONS.findIndex((item) => item.slug === direction.slug);
  const previous = PROTOTYPE_DIRECTIONS[(currentIndex - 1 + PROTOTYPE_DIRECTIONS.length) % PROTOTYPE_DIRECTIONS.length];
  const next = PROTOTYPE_DIRECTIONS[(currentIndex + 1) % PROTOTYPE_DIRECTIONS.length];
  const render = RENDERERS[direction.slug];

  return (
    <div className={styles.prototypePage} data-prototype={direction.slug}>
      <div className={styles.prototypeSwitcher}>
        <Link href="/prototypes" className={styles.switcherBack}><ArrowLeft size={15} /> All directions</Link>
        <div><span>{direction.number} / 08</span><strong>{direction.name}</strong></div>
        <nav aria-label="Switch prototype">
          <Link href={`/prototypes/${previous.slug}`} aria-label={`Previous: ${previous.name}`}><ArrowLeft size={16} /></Link>
          <Link href={`/prototypes/${next.slug}`} aria-label={`Next: ${next.name}`}><ArrowRight size={16} /></Link>
        </nav>
      </div>
      {render({ rows, preset, setPreset, paused, togglePause: () => setPaused((value) => !value) })}
    </div>
  );
}
