interface HermesLogoProps {
  className?: string;
}

export function HermesLogo({ className }: HermesLogoProps) {
  return (
    <div className={["hermes-logo", className].filter(Boolean).join(" ")} aria-label="Hermes Trader">
      <svg aria-hidden="true" className="hermes-mark" viewBox="0 0 48 48">
        <circle cx="24" cy="24" fill="currentColor" r="22" />
        <path
          d="M13 25.2c0-7.2 4.6-12.2 11.1-12.2 3.9 0 7.2 1.7 9.1 4.7l-4 2.6c-1.1-1.8-2.9-2.8-5.1-2.8-3.3 0-5.7 2.3-5.7 5.9 0 3.8 2.6 6.1 6.1 6.1 2.3 0 4.3-1 5.4-2.8l4 2.5c-2.1 3.1-5.5 4.8-9.5 4.8-6.7 0-11.4-4.7-11.4-11.8Z"
          fill="var(--mark-cutout)"
        />
        <path d="M28 13h5v21h-5z" fill="var(--mark-cutout)" />
        <path d="M22.7 21.4h10.4v4.4H22.7z" fill="var(--mark-cutout)" />
      </svg>
      <span className="hermes-wordmark">
        <strong>Hermes</strong>
        <span>Trader</span>
      </span>
    </div>
  );
}
