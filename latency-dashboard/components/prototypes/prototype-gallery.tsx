import Link from "next/link";
import { ArrowUpRight } from "lucide-react";
import { PROTOTYPE_DIRECTIONS } from "./prototype-data";
import styles from "./prototypes.module.css";

function Miniature({ slug }: { slug: string }) {
  return (
    <div className={`${styles.miniature} ${styles[`miniature_${slug}`]}`} aria-hidden="true">
      <span className={styles.miniNav} />
      <span className={styles.miniTitle} />
      <span className={styles.miniMetricA} />
      <span className={styles.miniMetricB} />
      <span className={styles.miniMetricC} />
      <span className={styles.miniRowA} />
      <span className={styles.miniRowB} />
      <span className={styles.miniRowC} />
    </div>
  );
}

export function PrototypeGallery() {
  return (
    <main className={styles.galleryRoot}>
      <header className={styles.galleryHeader}>
        <Link href="/" className={styles.galleryBrand}>Hermes Trader</Link>
        <p className={styles.galleryKicker}>Dashboard study · 01—08</p>
        <h1>Eight ways to<br />run the desk.</h1>
        <div className={styles.galleryIntro}>
          <p>These are working interface directions, not color themes. Each one reorganizes the same execution data around a different operator priority.</p>
          <p>Open them full-screen, try the presets, and pick the number whose structure feels right. Nothing here changes the live dashboard.</p>
        </div>
      </header>

      <section className={styles.galleryGrid} aria-label="Dashboard prototype directions">
        {PROTOTYPE_DIRECTIONS.map((direction) => (
          <Link
            href={`/prototypes/${direction.slug}`}
            className={styles.galleryCard}
            key={direction.slug}
          >
            <div className={styles.galleryCardTop}>
              <span>{direction.number}</span>
              <ArrowUpRight size={20} aria-hidden="true" />
            </div>
            <Miniature slug={direction.slug} />
            <h2>{direction.name}</h2>
            <p>{direction.strapline}</p>
            <dl>
              <div><dt>Density</dt><dd>{direction.density}</dd></div>
              <div><dt>Best for</dt><dd>{direction.bestFor}</dd></div>
            </dl>
          </Link>
        ))}
      </section>

      <footer className={styles.galleryFooter}>
        <span>Hermes Solana Operator Interface</span>
        <span>Prototype round · no production changes</span>
      </footer>
    </main>
  );
}
