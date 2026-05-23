import "dotenv/config";
import { readSubscriberRecords } from "./subscribers.js";
import { createSupabaseSubscriberRepository, importSubscribersToSupabase } from "./subscribers-supabase.js";

const supabaseUrl = process.env.SUPABASE_URL;
const supabaseServiceRoleKey = process.env.SUPABASE_SERVICE_ROLE_KEY;
const subscriberPath = process.argv[2] || process.env.TELEGRAM_SUBSCRIBERS_PATH || "data/telegram-subscribers.json";

if (!supabaseUrl || !supabaseServiceRoleKey) {
  throw new Error("SUPABASE_URL and SUPABASE_SERVICE_ROLE_KEY are required to import subscribers.");
}

const subscribers = await readSubscriberRecords({
  path: subscriberPath,
  initialChatIds: [process.env.TELEGRAM_CHAT_ID]
});

await importSubscribersToSupabase({
  repository: createSupabaseSubscriberRepository({
    url: supabaseUrl,
    serviceRoleKey: supabaseServiceRoleKey
  }),
  subscribers
});

const walletCount = subscribers.reduce((total, subscriber) => total + subscriber.watchedWallets.length, 0);
console.log(`Imported ${subscribers.length} subscriber(s) and ${walletCount} watched wallet(s) to Supabase.`);
