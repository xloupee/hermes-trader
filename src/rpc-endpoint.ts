const SENSITIVE_QUERY_KEY_PATTERN = /(?:api[-_]?key|token|secret|password|auth|signature|sig)/i;
const REDACTED_VALUE = "redacted";

function redactSensitiveQueryParams(url: URL): void {
  for (const key of [...url.searchParams.keys()]) {
    if (SENSITIVE_QUERY_KEY_PATTERN.test(key)) {
      url.searchParams.set(key, REDACTED_VALUE);
    }
  }
}

export function redactSolanaRpcUrl(rpcUrl: string): string {
  try {
    const url = new URL(rpcUrl);

    if (url.username) {
      url.username = REDACTED_VALUE;
    }

    if (url.password) {
      url.password = REDACTED_VALUE;
    }

    redactSensitiveQueryParams(url);
    return url.toString();
  } catch {
    return rpcUrl.replace(
      /((?:api[-_]?key|token|secret|password|auth|signature|sig)=)[^&\s]+/gi,
      `$1${REDACTED_VALUE}`
    );
  }
}

export function formatSolanaRpcEndpointLog(rpcUrl: string): string {
  return `Solana RPC endpoint | url=${redactSolanaRpcUrl(rpcUrl)}`;
}
