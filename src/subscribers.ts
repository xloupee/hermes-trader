import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname } from "node:path";
import { asRecord } from "./types.js";
import type { SubscriberStore, TelegramChatId } from "./types.js";

function normalizeChatId(chatId: TelegramChatId | undefined | null): string | null {
  return chatId === undefined || chatId === null ? null : String(chatId);
}

export function createSubscriberStore({
  path,
  initialChatIds = []
}: {
  path?: string;
  initialChatIds?: Array<TelegramChatId | undefined>;
}): SubscriberStore {
  const subscribers = new Set(initialChatIds.map(normalizeChatId).filter((chatId): chatId is string => Boolean(chatId)));
  let loaded = false;

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
      const chatIds = Array.isArray(data) ? data : record.chatIds;

      if (!Array.isArray(chatIds)) {
        return;
      }

      for (const chatId of chatIds) {
        const normalized = normalizeChatId(chatId as TelegramChatId);

        if (normalized) {
          subscribers.add(normalized);
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

  async function save(): Promise<void> {
    if (!path) {
      return;
    }

    await mkdir(dirname(path), { recursive: true });
    await writeFile(
      path,
      `${JSON.stringify({ chatIds: [...subscribers].sort(), updatedAt: new Date().toISOString() }, null, 2)}\n`
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

      if (normalized) {
        subscribers.add(normalized);
      }

      await save();
    },
    async remove(chatId) {
      await load();
      subscribers.delete(String(chatId));
      await save();
    },
    list() {
      return [...subscribers];
    },
    count() {
      return subscribers.size;
    }
  };
}
