export interface MetricItem {
  label: string;
  value: string | number;
}

export interface DetailItem {
  label: string;
  value: string;
}

export function MetricGrid({ items }: { items: MetricItem[] }) {
  return (
    <div className="dual-latency">
      {items.map((item) => (
        <Metric key={item.label} label={item.label} value={item.value} />
      ))}
    </div>
  );
}

export function MetricStrip({ items }: { items: MetricItem[] }) {
  return (
    <section className="metric-strip">
      {items.map((item) => (
        <Metric key={item.label} label={item.label} value={item.value} />
      ))}
    </section>
  );
}

export function DetailList({ items }: { items: DetailItem[] }) {
  return (
    <dl className="detail-list">
      {items.map((item) => (
        <Detail key={item.label} label={item.label} value={item.value} />
      ))}
    </dl>
  );
}

export function Metric({ label, value }: MetricItem) {
  return (
    <div className="metric">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

function Detail({ label, value }: DetailItem) {
  return (
    <>
      <dt>{label}</dt>
      <dd>{value}</dd>
    </>
  );
}
