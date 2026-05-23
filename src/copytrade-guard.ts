export function copyBuySubmissionKey({
  chatId,
  tradingWalletPublicKey,
  sourceWalletAddress,
  observedSignature
}: {
  chatId: string;
  tradingWalletPublicKey: string | null | undefined;
  sourceWalletAddress: string;
  observedSignature: string | null | undefined;
}): string | null {
  if (!tradingWalletPublicKey || !observedSignature) {
    return null;
  }

  return [chatId, tradingWalletPublicKey, sourceWalletAddress, observedSignature].join(":");
}

export function createCopyBuySubmissionGuard() {
  const active = new Set<string>();

  return {
    reserve(key: string | null): boolean {
      if (!key) {
        return true;
      }

      if (active.has(key)) {
        return false;
      }

      active.add(key);
      return true;
    },
    release(key: string | null): void {
      if (key) {
        active.delete(key);
      }
    },
    size(): number {
      return active.size;
    }
  };
}
