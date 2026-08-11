export const OPERATOR_SESSION_COOKIE: string;
export const OPERATOR_SESSION_TTL_SECONDS: number;
export function isOperatorShortcut(username: string, password: string): boolean;
export function createOperatorSessionToken(secret: string, nowMs?: number): Promise<string>;
export function verifyOperatorSessionToken(token: string | undefined, secret: string | undefined, nowMs?: number): Promise<boolean>;
