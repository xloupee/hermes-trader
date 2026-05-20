const TELEGRAM_API_BASE = "https://api.telegram.org";

async function callTelegramApi({ token, method, payload }) {
  assertTelegramConfig({ token, chatIdRequired: false });

  const response = await fetch(`${TELEGRAM_API_BASE}/bot${token}/${method}`, {
    method: payload ? "POST" : "GET",
    headers: payload
      ? {
          "content-type": "application/json"
        }
      : undefined,
    body: payload ? JSON.stringify(payload) : undefined
  });

  const body = await response.json().catch(() => null);

  if (!response.ok || body?.ok === false) {
    const description = body?.description || response.statusText;
    throw new Error(`Telegram ${method} failed: ${description}`);
  }

  return body;
}

export function assertTelegramConfig({ token, chatIdRequired = true, chatId }) {
  if (!token) {
    throw new Error("Missing TELEGRAM_BOT_TOKEN in environment");
  }

  if (chatIdRequired && !chatId) {
    throw new Error("Missing TELEGRAM_CHAT_ID in environment");
  }
}

export async function sendTelegramMessage({ token, chatId, text, replyMarkup }) {
  assertTelegramConfig({ token, chatId });

  const body = await callTelegramApi({
    token,
    method: "sendMessage",
    payload: {
      chat_id: chatId,
      text,
      parse_mode: "HTML",
      disable_web_page_preview: true,
      reply_markup: replyMarkup
    }
  });

  return body;
}

export async function sendTelegramPhoto({ token, chatId, photoUrl, caption, replyMarkup }) {
  assertTelegramConfig({ token, chatId });

  const body = await callTelegramApi({
    token,
    method: "sendPhoto",
    payload: {
      chat_id: chatId,
      photo: photoUrl,
      caption,
      parse_mode: "HTML",
      reply_markup: replyMarkup
    }
  });

  return body;
}

export async function getTelegramUpdates({ token, offset, timeout = 0 }) {
  assertTelegramConfig({ token, chatIdRequired: false });

  const search = new URLSearchParams({
    timeout: String(timeout),
    allowed_updates: JSON.stringify(["message", "channel_post"])
  });

  if (offset) {
    search.set("offset", String(offset));
  }

  const body = await callTelegramApi({
    token,
    method: `getUpdates?${search.toString()}`
  });

  return body.result || [];
}

export async function getTelegramBotInfo({ token }) {
  const body = await callTelegramApi({
    token,
    method: "getMe"
  });

  return body.result;
}

export async function clearTelegramWebhook({ token }) {
  const body = await callTelegramApi({
    token,
    method: "deleteWebhook",
    payload: {
      drop_pending_updates: false
    }
  });

  return body.result;
}

export async function setTelegramCommands({ token, commands }) {
  const body = await callTelegramApi({
    token,
    method: "setMyCommands",
    payload: {
      commands
    }
  });

  return body.result;
}
