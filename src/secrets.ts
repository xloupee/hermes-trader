import { createCipheriv, createDecipheriv, createHash, randomBytes } from "node:crypto";

const ENVELOPE_VERSION = "v1";
const IV_LENGTH = 12;
const AUTH_TAG_LENGTH = 16;
const MIN_SECRET_LENGTH = 32;

function keyFromSecret(secret: string): Buffer {
  const trimmed = secret.trim();

  if (trimmed.length < MIN_SECRET_LENGTH) {
    throw new Error(`PUMPPORTAL_WALLET_KEY_ENCRYPTION_SECRET must be at least ${MIN_SECRET_LENGTH} characters`);
  }

  return createHash("sha256").update(trimmed).digest();
}

export function encryptSecret(value: string, secret: string): string {
  const iv = randomBytes(IV_LENGTH);
  const cipher = createCipheriv("aes-256-gcm", keyFromSecret(secret), iv);
  const ciphertext = Buffer.concat([cipher.update(value, "utf8"), cipher.final()]);
  const tag = cipher.getAuthTag();

  return [ENVELOPE_VERSION, iv.toString("base64url"), tag.toString("base64url"), ciphertext.toString("base64url")].join(":");
}

export function decryptSecret(envelope: string, secret: string): string {
  const [version, encodedIv, encodedTag, encodedCiphertext] = envelope.split(":");

  if (version !== ENVELOPE_VERSION || !encodedIv || !encodedTag || !encodedCiphertext) {
    throw new Error("Unsupported encrypted secret format");
  }

  const decipher = createDecipheriv("aes-256-gcm", keyFromSecret(secret), Buffer.from(encodedIv, "base64url"), {
    authTagLength: AUTH_TAG_LENGTH
  });
  decipher.setAuthTag(Buffer.from(encodedTag, "base64url"));

  return Buffer.concat([decipher.update(Buffer.from(encodedCiphertext, "base64url")), decipher.final()]).toString("utf8");
}

export function encryptionSecretReady(secret: string | undefined): boolean {
  return Boolean(secret && secret.trim().length >= MIN_SECRET_LENGTH);
}
