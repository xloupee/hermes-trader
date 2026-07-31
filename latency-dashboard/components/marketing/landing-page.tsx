import { LandingHeroInteractive } from "@/components/marketing/landing-hero-interactive";
import styles from "@/components/marketing/landing-page.module.css";
import tokens from "@/components/marketing/landing-tokens.module.css";

export function LandingPage() {
  return (
    <main className={tokens.landingTokens}>
      <div className={styles.landing}>
        <a className={styles.skipLink} href="#main-copy">
          Skip to content
        </a>

        <section className={styles.landingInner} aria-label="Hermes Trader landing page" id="main-copy">
          <section className={styles.hero} id="top" data-landing-hero>
            <header className={styles.masthead}>
              <a className={styles.brand} href="#top" aria-label="Hermes Trader home">
                <span className={styles.brandWord}>HERMES TRADER</span>
              </a>

              <nav className={styles.navLinks} aria-label="Primary navigation">
                <a className={styles.navLink} href="#difference">
                  The difference
                </a>
                <a className={styles.navLink} href="#experience">
                  How it works
                </a>
              </nav>

              <a className={styles.accessLink} href="#access">
                Private access
              </a>
            </header>

            <LandingHeroInteractive />

            <div className={styles.heroStage}>
              <div className={styles.heroCopy}>
                <p className={styles.eyebrow}>Private execution · Solana</p>
                <h1 className={styles.heroTitle} id="hero-title">
                  Move when <em>they move.</em>
                </h1>
                <p className={styles.heroLead}>
                  Follow proven wallets and execute every move with speed, precision, and control.
                </p>

                <div className={styles.heroActions}>
                  <a className={styles.primaryAction} href="#access">
                    Explore private access <span aria-hidden="true" />
                  </a>
                  <a className={styles.textLink} href="#experience">
                    How Hermes Trader works
                  </a>
                </div>

                <div id="experience" className={styles.heroDispatch} aria-label="How Hermes Trader works">
                  <div className={styles.dispatchItem}>
                    <span className={styles.dispatchIndex}>01</span>
                    <strong>Follow</strong>
                    <small>Choose the signal.</small>
                  </div>
                  <div className={styles.dispatchItem}>
                    <span className={styles.dispatchIndex}>02</span>
                    <strong>Execute</strong>
                    <small>Act in the moment.</small>
                  </div>
                  <div className={styles.dispatchItem}>
                    <span className={styles.dispatchIndex}>03</span>
                    <strong>Control</strong>
                    <small>Stay in command.</small>
                  </div>
                </div>
              </div>
            </div>

            <aside className={styles.heroFolio} aria-hidden="true">
              Edition 01 · Solana
            </aside>
          </section>

          <section className={styles.editorial} id="difference" aria-labelledby="difference-title">
            <div className={styles.editorialInner}>
              <div className={styles.editorialGrid}>
                <div>
                  <p className={styles.sectionLabel}>01 / The difference</p>
                  <h2 className={styles.editorialTitle} id="difference-title">
                    Built for traders who refuse to arrive <em>late.</em>
                  </h2>
                </div>

                <ul className={styles.benefitList}>
                  <li className={styles.benefit}>
                    <span className={styles.benefitNumber}>01</span>
                    <div>
                      <h3>Signal over noise</h3>
                      <p>Follow the wallets you trust and leave the rest outside your strategy.</p>
                    </div>
                    <span className={styles.benefitArrow} aria-hidden="true" />
                  </li>
                  <li className={styles.benefit}>
                    <span className={styles.benefitNumber}>02</span>
                    <div>
                      <h3>Speed without friction</h3>
                      <p>Be ready when opportunity appears—not after the moment has passed.</p>
                    </div>
                    <span className={styles.benefitArrow} aria-hidden="true" />
                  </li>
                  <li className={styles.benefit}>
                    <span className={styles.benefitNumber}>03</span>
                    <div>
                      <h3>Control stays yours</h3>
                      <p>Choose who to follow, how to trade, and when to step away.</p>
                    </div>
                    <span className={styles.benefitArrow} aria-hidden="true" />
                  </li>
                </ul>
              </div>

              <div className={styles.accessNote} id="access">
                <div>
                  <p>Private beta</p>
                  <h3 className={styles.accessTitle}>Access opens in small cohorts.</h3>
                </div>
                <span className={styles.accessStatus}>Applications opening soon</span>
              </div>
            </div>
          </section>
        </section>

        <footer className={styles.footer}>
          <span>Hermes Trader · Private Solana execution</span>
          <span>Move with intent</span>
        </footer>
      </div>
    </main>
  );
}
