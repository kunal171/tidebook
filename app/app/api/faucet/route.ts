import { createHash } from "node:crypto";

import {
  createAssociatedTokenAccountIdempotentInstruction,
  createMintToInstruction,
  getAssociatedTokenAddressSync,
  getMint,
  TOKEN_PROGRAM_ID,
} from "@solana/spl-token";
import {
  clusterApiUrl,
  Connection,
  Keypair,
  PublicKey,
  sendAndConfirmTransaction,
  Transaction,
} from "@solana/web3.js";
import { NextRequest, NextResponse } from "next/server";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

type FaucetConfig = {
  authority: Keypair;
  baseMint: PublicKey;
  quoteMint: PublicKey;
  baseClaimAtoms: bigint;
  quoteClaimAtoms: bigint;
  rpcUrl: string;
  walletLimit: number;
  ipLimit: number;
  windowSeconds: number;
};

type Counter = { count: number; expiresAt: number };

// This fallback exists only for a single local development process. Vercel
// instances do not share memory, so production fails closed unless durable
// Upstash credentials are configured.
const localCounters = new Map<string, Counter>();

const RATE_LIMIT_SCRIPT = `
local wallet_count = redis.call("INCR", KEYS[1])
if wallet_count == 1 then redis.call("EXPIRE", KEYS[1], ARGV[3]) end
local ip_count = redis.call("INCR", KEYS[2])
if ip_count == 1 then redis.call("EXPIRE", KEYS[2], ARGV[3]) end
local wallet_ttl = redis.call("TTL", KEYS[1])
local ip_ttl = redis.call("TTL", KEYS[2])
local retry_after = math.max(wallet_ttl, ip_ttl)
if wallet_count > tonumber(ARGV[1]) or ip_count > tonumber(ARGV[2]) then
  return {0, retry_after}
end
return {1, retry_after}
`;

function requiredEnvironment(name: string): string {
  const value = process.env[name]?.trim();
  if (!value) {
    throw new Error(`${name} is not configured`);
  }
  return value;
}

function positiveInteger(name: string, fallback: number): number {
  const raw = process.env[name]?.trim();
  if (!raw) return fallback;

  const value = Number(raw);
  if (!Number.isSafeInteger(value) || value <= 0) {
    throw new Error(`${name} must be a positive safe integer`);
  }
  return value;
}

function positiveAtoms(name: string): bigint {
  const raw = requiredEnvironment(name);
  if (!/^[1-9][0-9]*$/.test(raw)) {
    throw new Error(`${name} must be a positive integer in atomic units`);
  }
  return BigInt(raw);
}

function readAuthority(): Keypair {
  const raw = requiredEnvironment("FAUCET_AUTHORITY_SECRET_KEY");
  let bytes: unknown;

  try {
    bytes = JSON.parse(raw);
  } catch {
    throw new Error("FAUCET_AUTHORITY_SECRET_KEY must be a JSON byte array");
  }

  if (
    !Array.isArray(bytes) ||
    bytes.length !== 64 ||
    bytes.some(
      (value) =>
        !Number.isInteger(value) || Number(value) < 0 || Number(value) > 255,
    )
  ) {
    throw new Error(
      "FAUCET_AUTHORITY_SECRET_KEY must contain exactly 64 byte values",
    );
  }

  return Keypair.fromSecretKey(Uint8Array.from(bytes as number[]));
}

function readConfig(): FaucetConfig {
  return {
    authority: readAuthority(),
    baseMint: new PublicKey(requiredEnvironment("FAUCET_BASE_MINT")),
    quoteMint: new PublicKey(requiredEnvironment("FAUCET_QUOTE_MINT")),
    baseClaimAtoms: positiveAtoms("FAUCET_BASE_CLAIM_ATOMS"),
    quoteClaimAtoms: positiveAtoms("FAUCET_QUOTE_CLAIM_ATOMS"),
    rpcUrl:
      process.env.FAUCET_SOLANA_RPC_URL?.trim() ||
      process.env.NEXT_PUBLIC_SOLANA_RPC_URL?.trim() ||
      clusterApiUrl("devnet"),
    walletLimit: positiveInteger("FAUCET_WALLET_LIMIT", 1),
    ipLimit: positiveInteger("FAUCET_IP_LIMIT", 5),
    windowSeconds: positiveInteger("FAUCET_WINDOW_SECONDS", 3_600),
  };
}

function requesterIp(request: NextRequest): string {
  return (
    request.headers.get("x-forwarded-for")?.split(",")[0]?.trim() ||
    request.headers.get("x-real-ip")?.trim() ||
    "unknown"
  );
}

function localIncrement(key: string, limit: number, windowSeconds: number) {
  const now = Date.now();
  const current = localCounters.get(key);
  const counter =
    !current || current.expiresAt <= now
      ? { count: 1, expiresAt: now + windowSeconds * 1_000 }
      : { count: current.count + 1, expiresAt: current.expiresAt };

  localCounters.set(key, counter);
  return {
    allowed: counter.count <= limit,
    retryAfter: Math.max(1, Math.ceil((counter.expiresAt - now) / 1_000)),
  };
}

async function enforceRateLimit(
  request: NextRequest,
  wallet: PublicKey,
  config: FaucetConfig,
) {
  const ipHash = createHash("sha256")
    .update(requesterIp(request))
    .digest("hex");
  const walletKey = `tidebook:faucet:wallet:${wallet.toBase58()}`;
  const ipKey = `tidebook:faucet:ip:${ipHash}`;
  const redisUrl = process.env.UPSTASH_REDIS_REST_URL?.replace(/\/$/, "");
  const redisToken = process.env.UPSTASH_REDIS_REST_TOKEN?.trim();

  if (!redisUrl || !redisToken) {
    const localFallbackEnabled =
      process.env.FAUCET_ALLOW_IN_MEMORY_RATE_LIMIT?.trim().toLowerCase() ===
      "true";

    // Vercel always requires shared durable state, regardless of this flag.
    if (process.env.VERCEL || !localFallbackEnabled) {
      throw new Error("Durable faucet rate limiting is not configured");
    }

    const walletResult = localIncrement(
      walletKey,
      config.walletLimit,
      config.windowSeconds,
    );
    const ipResult = localIncrement(
      ipKey,
      config.ipLimit,
      config.windowSeconds,
    );
    return {
      allowed: walletResult.allowed && ipResult.allowed,
      retryAfter: Math.max(walletResult.retryAfter, ipResult.retryAfter),
    };
  }

  const response = await fetch(redisUrl, {
    method: "POST",
    headers: {
      authorization: `Bearer ${redisToken}`,
      "content-type": "application/json",
    },
    body: JSON.stringify([
      "EVAL",
      RATE_LIMIT_SCRIPT,
      "2",
      walletKey,
      ipKey,
      String(config.walletLimit),
      String(config.ipLimit),
      String(config.windowSeconds),
    ]),
    cache: "no-store",
  });

  if (!response.ok) {
    throw new Error("Rate-limit service is unavailable");
  }

  const payload = (await response.json()) as { result?: unknown };
  if (!Array.isArray(payload.result) || payload.result.length !== 2) {
    throw new Error("Rate-limit service returned an invalid response");
  }

  return {
    allowed: Number(payload.result[0]) === 1,
    retryAfter: Math.max(1, Number(payload.result[1]) || config.windowSeconds),
  };
}

export async function GET() {
  try {
    const config = readConfig();
    const durableRateLimit = Boolean(
      process.env.UPSTASH_REDIS_REST_URL &&
      process.env.UPSTASH_REDIS_REST_TOKEN,
    );

    return NextResponse.json({
      enabled:
        durableRateLimit ||
        (!process.env.VERCEL &&
          process.env.FAUCET_ALLOW_IN_MEMORY_RATE_LIMIT?.trim().toLowerCase() ===
            "true"),
      baseMint: config.baseMint.toBase58(),
      quoteMint: config.quoteMint.toBase58(),
      baseClaimAtoms: config.baseClaimAtoms.toString(),
      quoteClaimAtoms: config.quoteClaimAtoms.toString(),
      windowSeconds: config.windowSeconds,
    });
  } catch {
    return NextResponse.json({ enabled: false });
  }
}

export async function POST(request: NextRequest) {
  try {
    const config = readConfig();
    const body = (await request.json()) as { wallet?: unknown };
    if (typeof body.wallet !== "string") {
      return NextResponse.json(
        { error: "A valid wallet address is required" },
        { status: 400 },
      );
    }

    let claimant: PublicKey;
    try {
      claimant = new PublicKey(body.wallet);
    } catch {
      return NextResponse.json(
        { error: "A valid wallet address is required" },
        { status: 400 },
      );
    }

    const rateLimit = await enforceRateLimit(request, claimant, config);
    if (!rateLimit.allowed) {
      return NextResponse.json(
        {
          error: "Faucet claim limit reached",
          retryAfter: rateLimit.retryAfter,
        },
        {
          status: 429,
          headers: { "retry-after": String(rateLimit.retryAfter) },
        },
      );
    }

    const connection = new Connection(config.rpcUrl, "confirmed");
    const [baseMint, quoteMint] = await Promise.all([
      getMint(connection, config.baseMint, "confirmed", TOKEN_PROGRAM_ID),
      getMint(connection, config.quoteMint, "confirmed", TOKEN_PROGRAM_ID),
    ]);
    for (const mint of [baseMint, quoteMint]) {
      if (!mint.mintAuthority?.equals(config.authority.publicKey)) {
        throw new Error(
          "The faucet account is not the configured mint authority",
        );
      }
    }

    const baseAccount = getAssociatedTokenAddressSync(
      config.baseMint,
      claimant,
    );
    const quoteAccount = getAssociatedTokenAddressSync(
      config.quoteMint,
      claimant,
    );
    const transaction = new Transaction().add(
      createAssociatedTokenAccountIdempotentInstruction(
        config.authority.publicKey,
        baseAccount,
        claimant,
        config.baseMint,
      ),
      createAssociatedTokenAccountIdempotentInstruction(
        config.authority.publicKey,
        quoteAccount,
        claimant,
        config.quoteMint,
      ),
      createMintToInstruction(
        config.baseMint,
        baseAccount,
        config.authority.publicKey,
        config.baseClaimAtoms,
      ),
      createMintToInstruction(
        config.quoteMint,
        quoteAccount,
        config.authority.publicKey,
        config.quoteClaimAtoms,
      ),
    );

    const signature = await sendAndConfirmTransaction(
      connection,
      transaction,
      [config.authority],
      { commitment: "confirmed" },
    );

    return NextResponse.json({ signature });
  } catch (cause) {
    console.error("Faucet claim failed", cause);
    return NextResponse.json(
      { error: "The faucet could not complete this claim" },
      { status: 503 },
    );
  }
}
