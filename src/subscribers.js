import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname } from "node:path";

function normalizeChatId(chatId) {
  return chatId === undefined || chatId === null ? null : String(chatId);
}

export function createSubscriberStore({ path, initialChatIds = [] }) {
  const subscribers = new Set(initialChatIds.map(normalizeChatId).filter(Boolean));
  let loaded = false;

  async function load() {
    if (loaded) {
      return;
    }

    loaded = true;

    if (!path) {
      return;
    }

    try {
      const body = await readFile(path, "utf8");
      const data = JSON.parse(body);
      const chatIds = Array.isArray(data) ? data : data.chatIds;

      for (const chatId of chatIds || []) {
        const normalized = normalizeChatId(chatId);

        if (normalized) {
          subscribers.add(normalized);
        }
      }
    } catch (error) {
      if (error.code !== "ENOENT") {
        console.warn(`Could not load Telegram subscribers: ${error.message}`);
      }
    }
  }

  async function save() {
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
      return subscribers.has(normalizeChatId(chatId));
    },
    async add(chatId) {
      await load();
      subscribers.add(normalizeChatId(chatId));
      await save();
    },
    async remove(chatId) {
      await load();
      subscribers.delete(normalizeChatId(chatId));
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
