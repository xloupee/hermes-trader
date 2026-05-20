import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname } from "node:path";
import { asRecord } from "./types.js";
import type { AlertModeValue, SubscriberRecord, SubscriberStore, TelegramChatId } from "./types.js";

const LEGACY_MODE: AlertModeValue = "migrations";

function normalizeChatId(chatId: TelegramChatId | undefined | null): string | null {
  return chatId === undefined || chatId === null ? null : String(chatId);
}

function normalizeMode(value: unknown): AlertModeValue | null {
  return value === "migrations" || value === "newtokens" || value === "both" ? value : null;
}

function makeSubscriber(chatId: string, mode: AlertModeValue | null, now = new Date().toISOString()): SubscriberRecord {
  return {
    chatId,
    mode,
    verifiedAt: now,
    updatedAt: now
  };
}

function mergeSubscriber(
  subscribers: Map<string, SubscriberRecord>,
  chatId: string,
  mode: AlertModeValue | null,
  verifiedAt?: unknown,
  updatedAt?: unknown
): void {
  const now = new Date().toISOString();
  const existing = subscribers.get(chatId);

  subscribers.set(chatId, {
    chatId,
    mode,
    verifiedAt: typeof verifiedAt === "string" ? verifiedAt : existing?.verifiedAt || now,
    updatedAt: typeof updatedAt === "string" ? updatedAt : existing?.updatedAt || now
  });
}

export function createSubscriberStore({
  path,
  initialChatIds = []
}: {
  path?: string;
  initialChatIds?: Array<TelegramChatId | undefined>;
}): SubscriberStore {
  const subscribers = new Map<string, SubscriberRecord>();
  let loaded = false;

  for (const chatId of initialChatIds) {
    const normalized = normalizeChatId(chatId);

    if (normalized) {
      mergeSubscriber(subscribers, normalized, LEGACY_MODE);
    }
  }

  async function load(): Promise<void> {
    if (loaded) {
      return;
    }

    loaded = true;

    if (!path) {
      return;
    }

    try {
      const body = await readFile(path, "utf8");
      const data = JSON.parse(body) as unknown;
      const record = asRecord(data);

      if (Array.isArray(data)) {
        loadLegacyChatIds(data);
        return;
      }

      if (Array.isArray(record.chatIds)) {
        loadLegacyChatIds(record.chatIds);
      }

      if (Array.isArray(record.subscribers)) {
        for (const entry of record.subscribers) {
          loadSubscriberRecord(entry);
        }
      }
    } catch (error) {
      if (error instanceof Error && "code" in error && error.code === "ENOENT") {
        return;
      }

      const message = error instanceof Error ? error.message : String(error);
      console.warn(`Could not load Telegram subscribers: ${message}`);
    }
  }

  function loadLegacyChatIds(chatIds: unknown[]): void {
    for (const chatId of chatIds) {
      const normalized = normalizeChatId(chatId as TelegramChatId);

      if (normalized) {
        mergeSubscriber(subscribers, normalized, LEGACY_MODE);
      }
    }
  }

  function loadSubscriberRecord(value: unknown): void {
    const record = asRecord(value);
    const chatId = normalizeChatId(record.chatId as TelegramChatId);

    if (!chatId) {
      return;
    }

    mergeSubscriber(subscribers, chatId, normalizeMode(record.mode), record.verifiedAt, record.updatedAt);
  }

  async function save(): Promise<void> {
    if (!path) {
      return;
    }

    await mkdir(dirname(path), { recursive: true });
    await writeFile(
      path,
      `${JSON.stringify(
        {
          subscribers: [...subscribers.values()].sort((left, right) => left.chatId.localeCompare(right.chatId)),
          updatedAt: new Date().toISOString()
        },
        null,
        2
      )}\n`
    );
  }

  return {
    async init() {
      await load();
      await save();
    },
    has(chatId) {
      return subscribers.has(String(chatId));
    },
    async add(chatId) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized) {
        return;
      }

      const existing = subscribers.get(normalized);

      if (existing) {
        subscribers.set(normalized, {
          ...existing,
          updatedAt: new Date().toISOString()
        });
      } else {
        subscribers.set(normalized, makeSubscriber(normalized, null));
      }

      await save();
    },
    async remove(chatId) {
      await load();
      subscribers.delete(String(chatId));
      await save();
    },
    get(chatId) {
      return subscribers.get(String(chatId)) || null;
    },
    async setMode(chatId, mode) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return false;
      }

      const existing = subscribers.get(normalized);
      subscribers.set(normalized, {
        ...(existing || makeSubscriber(normalized, mode)),
        mode,
        updatedAt: new Date().toISOString()
      });
      await save();
      return true;
    },
    list() {
      return [...subscribers.values()];
    },
    count() {
      return subscribers.size;
    }
  };
}
