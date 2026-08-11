export const OPERATOR_SESSION_COOKIE = "hermes_operator_session";
export const OPERATOR_SESSION_TTL_SECONDS = 60 * 60 * 24 * 7;

const encoder = new TextEncoder();

function bytesToBase64Url(bytes) {
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary).replaceAll("+", "-").replaceAll("/", "_").replace(/=+$/u, "");
}

function base64UrlToBytes(value) {
  const normalized = value.replaceAll("-", "+").replaceAll("_", "/");
  const padded = normalized.padEnd(Math.ceil(normalized.length / 4) * 4, "=");
  const binary = atob(padded);
  return Uint8Array.from(binary, (character) => character.charCodeAt(0));
}

async function signatureFor(payload, secret) {
  const key = await crypto.subtle.importKey(
    "raw",
    encoder.encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"]
  );
  return new Uint8Array(await crypto.subtle.sign("HMAC", key, encoder.encode(payload)));
}

function signaturesMatch(left, right) {
  if (left.length !== right.length) return false;
  let difference = 0;
  for (let index = 0; index < left.length; index += 1) difference |= left[index] ^ right[index];
  return difference === 0;
}

export function isOperatorShortcut(username, password) {
  return username === "123" && password === "123";
}

export async function createOperatorSessionToken(secret, nowMs = Date.now()) {
  if (!secret) throw new Error("operator session secret is not configured");
  const issuedAt = Math.floor(nowMs / 1000);
  const expiresAt = issuedAt + OPERATOR_SESSION_TTL_SECONDS;
  const payload = `v1.${issuedAt}.${expiresAt}`;
  const signature = await signatureFor(payload, secret);
  return `${payload}.${bytesToBase64Url(signature)}`;
}

export async function verifyOperatorSessionToken(token, secret, nowMs = Date.now()) {
  if (!token || !secret) return false;
  const parts = token.split(".");
  if (parts.length !== 4 || parts[0] !== "v1") return false;
  const issuedAt = Number(parts[1]);
  const expiresAt = Number(parts[2]);
  const now = Math.floor(nowMs / 1000);
  if (!Number.isSafeInteger(issuedAt) || !Number.isSafeInteger(expiresAt)) return false;
  if (issuedAt > now + 60 || expiresAt <= now || expiresAt - issuedAt !== OPERATOR_SESSION_TTL_SECONDS) return false;

  try {
    const expected = await signatureFor(parts.slice(0, 3).join("."), secret);
    return signaturesMatch(expected, base64UrlToBytes(parts[3]));
  } catch {
    return false;
  }
}
