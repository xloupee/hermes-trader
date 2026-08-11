"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import {
  createContext,
  type Dispatch,
  type ReactNode,
  type SetStateAction,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState
} from "react";
import { createConfigDiff, plannedExposure } from "@/lib/customer-config/diff.mjs";
import { fixtureConfigClient } from "@/lib/customer-config/client";
import { buildFixture } from "@/lib/customer-config/fixtures";
import type {
  ApplyLifecycle,
  ConfigActivity,
  ConfigDiff,
  CustomerConfig,
  FixtureScenario
} from "@/lib/customer-config/types";
import styles from "@/components/customer-app/customer-app.module.css";

interface CustomerConfigContextValue {
  savedConfig: CustomerConfig;
  draftConfig: CustomerConfig;
  setDraftConfig: Dispatch<SetStateAction<CustomerConfig>>;
  diffs: ConfigDiff[];
  activity: ConfigActivity[];
  scenario: FixtureScenario;
  lifecycle: ApplyLifecycle;
  message: string | null;
  reviewOpen: boolean;
  setReviewOpen: (open: boolean) => void;
  discardDraft: () => void;
  applyDraft: () => Promise<void>;
  selectScenario: (scenario: FixtureScenario) => Promise<void>;
}

const CustomerConfigContext = createContext<CustomerConfigContextValue | null>(null);

export function useCustomerConfig() {
  const context = useContext(CustomerConfigContext);
  if (!context) throw new Error("useCustomerConfig must be used inside CustomerAppShell");
  return context;
}

const scenarioLabels: Record<FixtureScenario, string> = {
  active: "Active",
  pending: "Publishing pending",
  failed: "Apply failed",
  conflict: "Revision conflict",
  empty: "No targets",
  unlinked: "Telegram unlinked",
  "missing-wallet": "No trading wallet"
};

const navigation = [
  { href: "/app", mark: "01", label: "Overview" },
  { href: "/app/copy-trading", mark: "02", label: "Copy trading" },
  { href: "/app/wallets", mark: "03", label: "Wallets" },
  { href: "/app/alerts", mark: "04", label: "Alerts" },
  { href: "/app/activity", mark: "05", label: "Activity" },
  { href: "/app/cashback", mark: "06", label: "Cashback" }
] as const;

function lifecycleForScenario(scenario: FixtureScenario): ApplyLifecycle {
  if (scenario === "pending") return "pending";
  if (scenario === "failed") return "failed";
  if (scenario === "conflict") return "conflict";
  return "active";
}

function runtimeSteps(lifecycle: ApplyLifecycle) {
  const definitions = [
    { id: "saved", title: "Draft saved", detail: "Desired revision recorded" },
    { id: "gateway", title: "Gateway applied", detail: "Execution settings loaded" },
    { id: "planner", title: "Planner applied", detail: "Targets and exits ready" },
    { id: "active", title: "Runtime active", detail: "Revision acknowledged" }
  ];

  if (lifecycle === "active" || lifecycle === "editing") return definitions.map((step) => ({ ...step, state: "complete" as const }));
  if (lifecycle === "applying") return definitions.map((step, index) => ({ ...step, state: index === 0 ? "current" as const : "waiting" as const }));
  if (lifecycle === "pending") return definitions.map((step, index) => ({ ...step, state: index < 2 ? "complete" as const : index === 2 ? "current" as const : "waiting" as const }));
  return definitions.map((step, index) => ({ ...step, state: index === 0 ? "complete" as const : index === 1 ? "error" as const : "waiting" as const }));
}

function CustomerConfigProvider({ children }: { children: ReactNode }) {
  const [initial] = useState(() => buildFixture("active"));
  const [savedConfig, setSavedConfig] = useState(initial.config);
  const [draftConfig, setDraftConfig] = useState(initial.config);
  const [activity, setActivity] = useState(initial.activity);
  const [scenario, setScenario] = useState<FixtureScenario>(initial.scenario);
  const [lifecycle, setLifecycle] = useState<ApplyLifecycle>("active");
  const [message, setMessage] = useState<string | null>(null);
  const [reviewOpen, setReviewOpen] = useState(false);

  useEffect(() => {
    let cancelled = false;
    fixtureConfigClient.load().then((state) => {
      if (cancelled) return;
      setScenario(state.scenario);
      setSavedConfig(state.config);
      setDraftConfig(state.config);
      setActivity(state.activity);
      setLifecycle(lifecycleForScenario(state.scenario));
    });
    return () => { cancelled = true; };
  }, []);

  const diffs = useMemo(() => createConfigDiff(savedConfig, draftConfig), [savedConfig, draftConfig]);

  useEffect(() => {
    if (diffs.length > 0 && lifecycle === "active") setLifecycle("editing");
    if (diffs.length === 0 && lifecycle === "editing") setLifecycle(lifecycleForScenario(scenario));
  }, [diffs.length, lifecycle, scenario]);

  function discardDraft() {
    setDraftConfig(savedConfig);
    setMessage(null);
    setLifecycle(lifecycleForScenario(scenario));
    setReviewOpen(false);
  }

  async function applyDraft() {
    if (diffs.length === 0) return;
    setLifecycle("applying");
    setMessage(null);
    const result = await fixtureConfigClient.apply({
      scenario,
      expectedRevision: savedConfig.revision,
      config: draftConfig
    });

    if (result.status === "conflict" || result.status === "failed") {
      setLifecycle(result.status);
      setMessage(result.message || "The demo operation did not become active.");
      return;
    }

    if (!result.config) return;
    const activityStatus: ConfigActivity["status"] = result.status === "pending" ? "pending" : "active";
    setSavedConfig(result.config);
    setDraftConfig(result.config);
    setLifecycle(result.status);
    setReviewOpen(false);
    setActivity((items) => [
      {
        id: result.operationId,
        type: "configuration",
        title: activityStatus === "active" ? `Revision ${result.config?.revision} became active` : `Revision ${result.config?.revision} is publishing`,
        detail: `${diffs.length} demo ${diffs.length === 1 ? "change" : "changes"} applied locally.`,
        status: activityStatus,
        occurredAt: "Just now"
      },
      ...items
    ]);
  }

  async function selectScenario(nextScenario: FixtureScenario) {
    const state = await fixtureConfigClient.selectScenario(nextScenario);
    setScenario(state.scenario);
    setSavedConfig(state.config);
    setDraftConfig(state.config);
    setActivity(state.activity);
    setLifecycle(lifecycleForScenario(nextScenario));
    setMessage(null);
    setReviewOpen(false);
  }

  const value = useMemo<CustomerConfigContextValue>(() => ({
    savedConfig,
    draftConfig,
    setDraftConfig,
    diffs,
    activity,
    scenario,
    lifecycle,
    message,
    reviewOpen,
    setReviewOpen,
    discardDraft,
    applyDraft,
    selectScenario
  }), [activity, diffs, draftConfig, lifecycle, message, reviewOpen, savedConfig, scenario]);

  return <CustomerConfigContext.Provider value={value}>{children}</CustomerConfigContext.Provider>;
}

function Brand() {
  return (
    <div className={styles.brand}>
      <span className={styles.brandMark} aria-hidden="true">H</span>
      <div><strong>Hermes</strong><small>Personal trading</small></div>
    </div>
  );
}

function Navigation() {
  const pathname = usePathname();
  return (
    <nav className={styles.navigation} aria-label="Customer application">
      {navigation.map((item) => {
        const active = item.href === "/app" ? pathname === item.href : pathname.startsWith(item.href);
        return (
          <Link key={item.href} href={item.href} className={active ? styles.navActive : undefined} aria-current={active ? "page" : undefined}>
            <span>{item.mark}</span>{item.label}
          </Link>
        );
      })}
    </nav>
  );
}

function ActivationRail() {
  const { draftConfig, diffs, lifecycle, message, scenario, selectScenario, setReviewOpen } = useCustomerConfig();
  const steps = runtimeSteps(lifecycle);

  return (
    <aside className={styles.activationRail} aria-label="Configuration activation status">
      <div className={styles.railHeader}>
        <strong>Activation rail</strong>
        <span className={`${styles.statusPill} ${styles[`status_${lifecycle}`]}`}>{lifecycle === "editing" ? "Draft" : lifecycle}</span>
      </div>
      <label className={styles.scenarioPicker}>
        <span>Fixture scenario</span>
        <select value={scenario} onChange={(event) => selectScenario(event.target.value as FixtureScenario)}>
          {Object.entries(scenarioLabels).map(([value, label]) => <option key={value} value={value}>{label}</option>)}
        </select>
      </label>
      <div className={styles.runtimeSteps}>
        {steps.map((step) => (
          <div key={step.id} className={`${styles.runtimeStep} ${styles[`runtime_${step.state}`]}`}>
            <i aria-hidden="true">{step.state === "complete" ? "✓" : step.state === "error" ? "!" : ""}</i>
            <div><strong>{step.title}</strong><span>{step.detail}</span></div>
          </div>
        ))}
      </div>
      <div className={styles.railSummary}>
        <span>Active revision</span><strong>{draftConfig.revision}</strong>
        <span>Planned exposure</span><strong>{plannedExposure(draftConfig).toFixed(2)} SOL</strong>
      </div>
      <div className={styles.stagedChanges}>
        <p>Staged locally</p>
        {diffs.length === 0 ? <span className={styles.noChanges}>No unpublished changes.</span> : diffs.slice(0, 3).map((diff) => (
          <div key={diff.id}><strong>{diff.label}</strong><span>{diff.before} → {diff.after}</span></div>
        ))}
        {diffs.length > 3 ? <span className={styles.moreChanges}>+{diffs.length - 3} more changes</span> : null}
      </div>
      {message ? <p className={styles.railError} role="alert">{message}</p> : null}
      <button className={styles.primaryButton} type="button" disabled={diffs.length === 0 || lifecycle === "applying"} onClick={() => setReviewOpen(true)}>
        {lifecycle === "applying" ? "Publishing draft…" : `Review ${diffs.length || "no"} ${diffs.length === 1 ? "change" : "changes"}`}
      </button>
    </aside>
  );
}

function ReviewDialog() {
  const { applyDraft, diffs, discardDraft, draftConfig, lifecycle, message, reviewOpen, setReviewOpen } = useCustomerConfig();
  const dialogRef = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    const dialog = dialogRef.current;
    if (!dialog) return;
    if (reviewOpen && !dialog.open) dialog.showModal();
    if (!reviewOpen && dialog.open) dialog.close();
  }, [reviewOpen]);

  return (
    <dialog ref={dialogRef} className={styles.reviewDialog} onClose={() => setReviewOpen(false)}>
      <div className={styles.reviewHeader}>
        <div><p>Demo revision {draftConfig.revision + 1}</p><h2>Review changes</h2></div>
        <button type="button" className={styles.closeButton} onClick={() => setReviewOpen(false)} aria-label="Close review">×</button>
      </div>
      <p className={styles.reviewIntro}>Confirm the account, trading wallet, and economic effect. Applying only updates this local fixture.</p>
      <div className={styles.reviewContext}>
        <span>Account</span><strong>{draftConfig.telegramHandle}</strong>
        <span>Trading wallet</span><strong>{draftConfig.tradingWallet?.address || "No wallet"}</strong>
        <span>Scope</span><strong>Future copied buys only</strong>
        <span>Maximum planned exposure</span><strong>{plannedExposure(draftConfig).toFixed(2)} SOL</strong>
      </div>
      <div className={styles.reviewDiffs}>
        {diffs.map((diff) => (
          <div key={diff.id} className={styles.reviewDiff}>
            <div><strong>{diff.label}</strong><span>{diff.group}</span>{diff.warning ? <em>{diff.warning}</em> : null}</div>
            <p><del>{diff.before}</del><b>{diff.after}</b></p>
          </div>
        ))}
      </div>
      {message ? <p className={styles.dialogError} role="alert">{message}</p> : null}
      <div className={styles.reviewActions}>
        <button className={styles.primaryButton} type="button" onClick={applyDraft} disabled={lifecycle === "applying"}>{lifecycle === "applying" ? "Applying locally…" : `Apply ${diffs.length} ${diffs.length === 1 ? "change" : "changes"}`}</button>
        <button className={styles.secondaryButton} type="button" onClick={() => setReviewOpen(false)}>Keep editing</button>
        <button className={styles.textButton} type="button" onClick={discardDraft}>Discard draft</button>
      </div>
    </dialog>
  );
}

function MobileDraftBar() {
  const { diffs, setReviewOpen } = useCustomerConfig();
  if (diffs.length === 0) return null;
  return (
    <div className={styles.mobileDraftBar}>
      <div><strong>{diffs.length} draft {diffs.length === 1 ? "change" : "changes"}</strong><span>Nothing changes until you apply</span></div>
      <button type="button" onClick={() => setReviewOpen(true)}>Review</button>
    </div>
  );
}

function CustomerAppLayout({ children }: { children: ReactNode }) {
  const pathname = usePathname();
  if (pathname === "/app/welcome") return <main className={styles.welcomeFrame}>{children}</main>;

  return (
    <div className={styles.shell}>
      <a className={styles.skipLink} href="#customer-main">Skip to main content</a>
      <aside className={styles.sidebar}>
        <Brand />
        <Navigation />
        <Link className={styles.operatorLink} href="/dashboard">← Operator panel</Link>
        <Link className={styles.accountLink} href="/app/account"><strong>Kenneth</strong><span>Telegram account</span></Link>
      </aside>
      <header className={styles.mobileHeader}><Brand /><span className={styles.demoPill}>Demo data</span></header>
      <main id="customer-main" className={styles.main}>{children}</main>
      <ActivationRail />
      <MobileDraftBar />
      <nav className={styles.mobileNav} aria-label="Mobile customer navigation">
        {navigation.slice(0, 4).map((item) => {
          const active = item.href === "/app" ? pathname === item.href : pathname.startsWith(item.href);
          return <Link key={item.href} href={item.href} className={active ? styles.mobileNavActive : undefined} aria-current={active ? "page" : undefined}><b>{item.mark}</b>{item.label.replace("Copy trading", "Trade")}</Link>;
        })}
        <Link href="/app/account" className={pathname.startsWith("/app/account") ? styles.mobileNavActive : undefined} aria-current={pathname.startsWith("/app/account") ? "page" : undefined}><b>05</b>More</Link>
      </nav>
      <ReviewDialog />
    </div>
  );
}

export function CustomerAppShell({ children }: { children: ReactNode }) {
  return <CustomerConfigProvider><CustomerAppLayout>{children}</CustomerAppLayout></CustomerConfigProvider>;
}
