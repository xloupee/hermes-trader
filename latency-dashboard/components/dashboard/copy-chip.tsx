"use client";

import { useState } from "react";
import { Copy } from "lucide-react";

import styles from "@/components/dashboard/dashboard-shared.module.css";
import { shortText } from "@/lib/dashboard-client";

export function CopyChip({
  value,
  label
}: {
  value: string | null | undefined;
  label: string;
}) {
  const [feedback, setFeedback] = useState<string | null>(null);
  const display = shortText(value, 7);

  async function copyValue() {
    if (!value) {
      return;
    }
    try {
      await navigator.clipboard.writeText(value);
      setFeedback("copied");
      window.setTimeout(() => setFeedback(null), 1600);
    } catch {
      setFeedback("copy failed");
      window.setTimeout(() => setFeedback(null), 1600);
    }
  }

  return (
    <span className={styles.copyCell}>
      <span>{value ? display : "n/a"}</span>
      <button
        className={styles.copyButton}
        onClick={copyValue}
        type="button"
        title={`Copy ${label}`}
        aria-label={`Copy ${label}`}
        disabled={!value}
      >
        <Copy size={13} />
      </button>
      {feedback ? <em className={styles.copyToast}>{feedback}</em> : null}
    </span>
  );
}
