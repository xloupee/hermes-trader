import "dotenv/config";
import { getTelegramUpdates } from "./telegram.js";

const updates = await getTelegramUpdates({
  token: process.env.TELEGRAM_BOT_TOKEN
});

const chats = new Map();

for (const update of updates) {
  const chat = update.message?.chat || update.channel_post?.chat;

  if (chat?.id) {
    chats.set(chat.id, {
      id: chat.id,
      type: chat.type,
      title: chat.title,
      username: chat.username,
      firstName: chat.first_name,
      lastName: chat.last_name
    });
  }
}

if (chats.size === 0) {
  console.log("No chats found. Send a message to your bot in Telegram, then run npm run chat-id again.");
} else {
  console.log("Found Telegram chats:");
  for (const chat of chats.values()) {
    console.log(JSON.stringify(chat, null, 2));
  }
}
