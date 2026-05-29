import assert from "node:assert/strict";
import test from "node:test";
import { buildMigrationReplyMarkup, extractMigrationData, formatMigrationMessage } from "../dist/format.js";

const mint = "6S1vouzmAsvCTCy9Q93mTN817AJQLmTWnWCD3GfDpump";
const signature = "5".repeat(88);
const baseEvent = {
  txType: "migrate",
  mint,
  signature,
  pool: "pump-amm"
};
const baseConfig = {
  pumpFunBaseUrl: "https://pump.fun",
  solscanBaseUrl: "https://solscan.io"
};
const copyCaRow = [
  {
    text: "📋 Copy CA",
    copy_text: {
      text: mint
    }
  }
];

function config(extra = {}) {
  return {
    ...baseConfig,
    ...extra
  };
}

test("migration formatter adds metadata social buttons after Copy CA", () => {
  const migrationConfig = config({
    metadata: {
      twitter: " https://x.com/token_alpha ",
      telegram: "@token_alpha",
      website: "https://token.example/home"
    },
    tokenInfo: {
      twitter: "https://x.com/token_fallback",
      telegram: "https://t.me/token_fallback",
      website: "https://fallback.example/home"
    }
  });
  const migration = extractMigrationData(baseEvent, migrationConfig);
  const markup = buildMigrationReplyMarkup(baseEvent, migrationConfig);

  assert.deepEqual(migration.socialLinks, {
    twitterUrl: "https://x.com/token_alpha",
    telegramUrl: "https://t.me/token_alpha",
    websiteUrl: "https://token.example/home"
  });
  assert.deepEqual(markup?.inline_keyboard, [
    copyCaRow,
    [
      { text: "X", url: "https://x.com/token_alpha" },
      { text: "Telegram", url: "https://t.me/token_alpha" },
      { text: "Website", url: "https://token.example/home" }
    ]
  ]);
});

test("migration formatter falls back to Pump.fun coin-info socials", () => {
  const migrationConfig = config({
    metadata: {
      twitter: "",
      telegram: "",
      website: ""
    },
    tokenInfo: {
      xUrl: "https://x.com/token_info",
      telegram: "t.me/token_info",
      external_url: "https://token-info.example/home"
    }
  });
  const migration = extractMigrationData(baseEvent, migrationConfig);
  const markup = buildMigrationReplyMarkup(baseEvent, migrationConfig);

  assert.deepEqual(migration.socialLinks, {
    twitterUrl: "https://x.com/token_info",
    telegramUrl: "https://t.me/token_info",
    websiteUrl: "https://token-info.example/home"
  });
  assert.deepEqual(markup?.inline_keyboard[1], [
    { text: "X", url: "https://x.com/token_info" },
    { text: "Telegram", url: "https://t.me/token_info" },
    { text: "Website", url: "https://token-info.example/home" }
  ]);
});

test("migration formatter omits empty and malformed social links", () => {
  const migrationConfig = config({
    metadata: {
      twitter: "javascript:alert(1)",
      telegram: "telegram://resolve?domain=bad",
      website: "ftp://example.com/token"
    },
    tokenInfo: {
      twitterUrl: "",
      telegramUrl: "",
      externalUrl: ""
    }
  });
  const migration = extractMigrationData(baseEvent, migrationConfig);
  const markup = buildMigrationReplyMarkup(baseEvent, migrationConfig);
  const message = formatMigrationMessage(baseEvent, migrationConfig);

  assert.deepEqual(migration.socialLinks, {
    twitterUrl: null,
    telegramUrl: null,
    websiteUrl: null
  });
  assert.deepEqual(markup?.inline_keyboard, [copyCaRow]);
  assert.doesNotMatch(message, /javascript:|telegram:\/\/|ftp:\/\//);
});

test("migration formatter leaves coins without socials on the existing copy CA layout", () => {
  const migration = extractMigrationData(baseEvent, baseConfig);
  const markup = buildMigrationReplyMarkup(baseEvent, baseConfig);
  const message = formatMigrationMessage(baseEvent, baseConfig);

  assert.deepEqual(migration.socialLinks, {
    twitterUrl: null,
    telegramUrl: null,
    websiteUrl: null
  });
  assert.deepEqual(markup?.inline_keyboard, [copyCaRow]);
  assert.doesNotMatch(message, /<b>Socials<\/b>|<b>Social Links<\/b>/);
});
