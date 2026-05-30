import { asRecord, isRecord } from "./types.js";
import type { TelegramBotInfo, TelegramChatId, TelegramMessage, TelegramReplyMarkup, TelegramUpdate } from "./types.js";

const TELEGRAM_API_BASE = "https://api.telegram.org";

interface TelegramApiResponse<TResult> {
  ok?: boolean;
  result?: TResult;
  description?: string;
}

interface TelegramApiOptions {
  token?: string;
  method: string;
  payload?: Record<string, unknown>;
}

interface TelegramConfigOptions {
  token?: string;
  chatIdRequired?: boolean;
  chatId?: TelegramChatId;
}

async function callTelegramApi<TResult>({ token, method, payload }: TelegramApiOptions): Promise<TelegramApiResponse<TResult>> {
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

  const body = asRecord(await response.json().catch(() => null));

  if (!response.ok || body.ok === false) {
    const description = typeof body.description === "string" ? body.description : response.statusText;
    throw new Error(`Telegram ${method} failed: ${description}`);
  }

  return body as TelegramApiResponse<TResult>;
}

export function assertTelegramConfig({ token, chatIdRequired = true, chatId }: TelegramConfigOptions): void {
  if (!token) {
    throw new Error("Missing TELEGRAM_BOT_TOKEN in environment");
  }

  if (chatIdRequired && !chatId) {
    throw new Error("Missing TELEGRAM_CHAT_ID in environment");
  }
}

interface SendTelegramMessageOptions {
  token?: string;
  chatId?: TelegramChatId;
  text: string;
  replyMarkup?: TelegramReplyMarkup;
  replyToMessageId?: number | null;
}

export async function sendTelegramMessage({
  token,
  chatId,
  text,
  replyMarkup,
  replyToMessageId
}: SendTelegramMessageOptions): Promise<TelegramApiResponse<TelegramMessage>> {
  assertTelegramConfig({ token, chatId });

  const body = await callTelegramApi<TelegramMessage>({
    token,
    method: "sendMessage",
    payload: {
      chat_id: chatId,
      text,
      parse_mode: "HTML",
      disable_web_page_preview: true,
      reply_to_message_id: replyToMessageId || undefined,
      allow_sending_without_reply: true,
      reply_markup: replyMarkup
    }
  });

  return body;
}

interface SendTelegramPhotoOptions {
  token?: string;
  chatId?: TelegramChatId;
  photoUrl: string;
  caption: string;
  replyMarkup?: TelegramReplyMarkup;
}

export async function sendTelegramPhoto({
  token,
  chatId,
  photoUrl,
  caption,
  replyMarkup
}: SendTelegramPhotoOptions): Promise<TelegramApiResponse<unknown>> {
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

interface GetTelegramUpdatesOptions {
  token?: string;
  offset?: number;
  timeout?: number;
}

export async function getTelegramUpdates({ token, offset, timeout = 0 }: GetTelegramUpdatesOptions): Promise<TelegramUpdate[]> {
  assertTelegramConfig({ token, chatIdRequired: false });

  const search = new URLSearchParams({
    timeout: String(timeout),
    allowed_updates: JSON.stringify(["message", "channel_post", "callback_query"])
  });

  if (offset) {
    search.set("offset", String(offset));
  }

  const body = await callTelegramApi<unknown>({
    token,
    method: `getUpdates?${search.toString()}`
  });

  return Array.isArray(body.result) ? (body.result as TelegramUpdate[]) : [];
}

export async function answerTelegramCallbackQuery({
  token,
  callbackQueryId,
  text
}: {
  token?: string;
  callbackQueryId: string;
  text?: string;
}): Promise<TelegramApiResponse<unknown>> {
  return callTelegramApi({
    token,
    method: "answerCallbackQuery",
    payload: {
      callback_query_id: callbackQueryId,
      text
    }
  });
}

export async function getTelegramBotInfo({ token }: { token?: string }): Promise<TelegramBotInfo> {
  const body = await callTelegramApi<unknown>({
    token,
    method: "getMe"
  });

  return isRecord(body.result) ? (body.result as TelegramBotInfo) : {};
}

export async function clearTelegramWebhook({ token }: { token?: string }): Promise<unknown> {
  const body = await callTelegramApi<unknown>({
    token,
    method: "deleteWebhook",
    payload: {
      drop_pending_updates: false
    }
  });

  return body.result;
}

export async function setTelegramCommands({
  token,
  commands
}: {
  token?: string;
  commands: Array<{ command: string; description: string }>;
}): Promise<unknown> {
  const body = await callTelegramApi<unknown>({
    token,
    method: "setMyCommands",
    payload: {
      commands
    }
  });

  return body.result;
}
