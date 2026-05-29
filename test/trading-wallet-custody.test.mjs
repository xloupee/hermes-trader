import assert from "node:assert/strict";
import test from "node:test";
import {
  decryptLocalSolanaKeypair,
  decryptSecret,
  encryptSecret,
  generateLocalSolanaHotWallet
} from "../dist/secrets.js";
import { normalizeTradingWallet } from "../dist/subscribers.js";

const encryptionSecret = "custody-v2-test-secret-12345678901234567890";

test("legacy PumpPortal trading wallets default to PumpPortal Lightning custody", () => {
  const wallet = normalizeTradingWallet({
    publicKey: "PumpPortalWallet111111111111111111111111111",
    encryptedApiKey: "encrypted-api-key",
    apiKeyLast4: "ikey",
    createdAt: "2026-05-23T00:00:00.000Z",
    updatedAt: "2026-05-23T00:00:00.000Z"
  });

  assert.equal(wallet?.provider, "pumpportal-lightning");
  assert.equal(wallet?.kind, "pumpportal-lightning");
  assert.equal(wallet?.encryptedApiKey, "encrypted-api-key");
  assert.equal(wallet?.apiKeyLast4, "ikey");
});

test("local Solana wallet helpers generate and decrypt a Keypair without serializing plaintext secrets", () => {
  const generated = generateLocalSolanaHotWallet();
  const encryptedSecretKey = encryptSecret(generated.secretKey, encryptionSecret);
  const wallet = normalizeTradingWallet({
    publicKey: generated.publicKey,
    provider: "local-solana",
    kind: "local-solana",
    encryptedSecretKey,
    secretKeyFormat: generated.secretKeyFormat,
    keyLast4: generated.secretKey.slice(-4),
    createdAt: "2026-05-28T00:00:00.000Z",
    updatedAt: "2026-05-28T00:00:00.000Z"
  });
  const decrypted = decryptLocalSolanaKeypair({
    encryptedSecretKey: wallet?.encryptedSecretKey || "",
    encryptionSecret,
    secretKeyFormat: wallet?.secretKeyFormat
  });

  assert.equal(generated.secretKeyFormat, "base58");
  assert.equal(decrypted.publicKey.toBase58(), generated.publicKey);
  assert.equal(decryptSecret(encryptedSecretKey, encryptionSecret), generated.secretKey);
  assert.equal(wallet?.provider, "local-solana");
  assert.equal(wallet?.kind, "local-solana");
  assert.equal(wallet?.encryptedApiKey, "");
  assert.equal(wallet?.encryptedSecretKey, encryptedSecretKey);
  assert.equal(JSON.stringify(wallet).includes(generated.secretKey), false);
});

test("local Solana wallet decryption keeps legacy base64 secrets working", () => {
  const generated = generateLocalSolanaHotWallet();
  const keypair = decryptLocalSolanaKeypair({
    encryptedSecretKey: encryptSecret(generated.secretKey, encryptionSecret),
    encryptionSecret,
    secretKeyFormat: generated.secretKeyFormat
  });
  const legacyBase64Secret = Buffer.from(keypair.secretKey).toString("base64");
  const decrypted = decryptLocalSolanaKeypair({
    encryptedSecretKey: encryptSecret(legacyBase64Secret, encryptionSecret),
    encryptionSecret,
    secretKeyFormat: "base64"
  });

  assert.equal(decrypted.publicKey.toBase58(), generated.publicKey);
});

test("local Solana wallet decryption fails closed for missing or malformed key material", () => {
  assert.throws(
    () => decryptLocalSolanaKeypair({
      encryptedSecretKey: "",
      encryptionSecret,
      secretKeyFormat: "base64"
    }),
    /Unsupported encrypted secret format/
  );

  assert.throws(
    () => decryptLocalSolanaKeypair({
      encryptedSecretKey: encryptSecret("not-base64-keypair", encryptionSecret),
      encryptionSecret,
      secretKeyFormat: "base64"
    }),
    /bad secret key size/
  );
});
