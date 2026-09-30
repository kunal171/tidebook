# Tidebook App

The Tidebook web application is a Next.js App Router, React, and TypeScript
client for the Anchor program.

## Run locally

```bash
npm install
npm run dev
```

Create a production build with:

```bash
npm run build
```

## Current UI milestone

- Connect a browser wallet through Solana Wallet Adapter.
- Connect to Solana devnet.
- Discover active and paused markets without connecting a wallet.
- Display the connected wallet's aggregate base and quote SPL balances beside
  its market-specific internal free and locked balances.
- Enter deposits, withdrawals, prices, and quantities in human token units;
  the client performs exact decimal-to-atomic conversion before signing.
- Create markets as an active protocol administrator.
- Submit bids and asks that either cross up to three best-price FIFO makers in
  one atomic transaction or rest as collateralized limit orders, including
  empty, best, middle, worst, and same-price FIFO insertion paths.
- Atomically post a valid taker remainder when the bounded path exhausts the
  crossing book or reaches a non-crossing price; keep it free only when more
  crossing liquidity remains beyond the cap or its bid notional rounds to zero.
- List wallet-owned open, filled, and canceled orders; only open orders expose
  cancellation, including while a market is paused.
- Pause, unpause, and safely close markets as their authority.
- Manage protocol administrators as the super-admin.
- Provide a server-backed, rate-limited faucet that mints fixed amounts of both
  valueless assets directly into any connected wallet's associated token
  accounts.

The checked-in client IDL mirrors the generated Anchor IDL. After any account
layout or instruction change, rebuild and redeploy the program before using the
updated client against devnet.

Configure the public program identity and optionally override the default
devnet RPC endpoint:

```bash
NEXT_PUBLIC_TIDEBOOK_PROGRAM_ID=AG4ztZvcZjzFKjoXLzH8M6U3if9CwVFTXH2FHjqg7SdL
NEXT_PUBLIC_SOLANA_RPC_URL=https://your-devnet-endpoint.example
```

Copy `.env.example` to `app/.env.local` or add the variables to the existing
ignored `app/.env`. Configure the same public program-ID variable in Vercel.
Next.js inlines `NEXT_PUBLIC_*` values during its build, so changing the program
identity requires restarting the local server or rebuilding the deployment.

## Test faucet safety

The active web faucet is server-backed. Its authority key is loaded only by the
Next.js server route; it is never sent to the browser or stored in Git. A claim
creates missing associated token accounts and mints fixed amounts directly to
the requesting wallet, so the server does not maintain a transferable token
inventory.

Claims are limited by both wallet and hashed IP address. Vercel fails closed
unless `UPSTASH_REDIS_REST_URL` and `UPSTASH_REDIS_REST_TOKEN` provide shared
durable counters. The process-local limiter is an explicit local-development
fallback and must never be enabled on Vercel. These limits are abuse friction,
not Sybil resistance: a determined caller can rotate wallets and networks.

The current devnet pair claims `10 tBTC` and `1,000 tUSDT` once per wallet per
hour, with at most five claims per IP in the same window. These assets have no
value and must never be bridged, sold, or treated as production tokens.

The program-controlled PDA faucet remains an alternative research design, but
a mint must use exactly one authority model at a time. Do not initialize the
on-chain faucet after transferring authority to the server account.

Full bootstrap steps and address placeholders live in `docs/deployment.md` and
`docs/devnet-addresses.md`.

## Multi-maker devnet smoke test

`npm run devnet:multi-maker` executes a one-shot current-layout smoke test. It
creates the market and internal balances, deposits test assets, places two asks
at one FIFO level, submits two matching instructions in one transaction, and
asserts the final orders, market pointers, level closure, and exact balances.

The runner intentionally does not create or mint SPL assets. Supply fresh
no-value devnet mints, temporary signer files, and their funded token accounts:

```bash
SOLANA_WALLET=/path/to/deployer.json \
MAKER_TWO_WALLET=/path/to/second-maker.json \
TAKER_WALLET=/path/to/taker.json \
BASE_MINT=<base-mint> \
QUOTE_MINT=<quote-mint> \
MAKER_ONE_BASE_TOKEN=<token-account> \
MAKER_TWO_BASE_TOKEN=<token-account> \
TAKER_QUOTE_TOKEN=<token-account> \
npm run devnet:multi-maker
```

Use a fresh mint pair for each run because the market PDA is deterministic and
the script deliberately tests initialization as part of the complete flow.

## Application boundaries

- `app/layout.tsx` and route pages are Server Components by default.
- `components/solana-provider.tsx` owns the client-only wallet and RPC context.
- `components/` contains the client-side route workspaces and transaction flows.
- `lib/tidebook.ts` owns account types, PDA derivation, token-account discovery,
  price-level traversal, formatting, and Anchor program construction.
- `lib/solana.ts` owns shared network and program-identity configuration.

Price-level traversal begins at the market best price and follows
`worse_price` links, so transaction construction is currently O(number of price
levels). Those RPC reads are advisory: the on-chain instruction revalidates all
neighbor accounts atomically, and a stale transaction must refresh and retry.
An indexer can later accelerate discovery without changing the program API.

Crossing-order construction is isolated in `lib/matching-plan.ts`. It starts at
the opposing best price, validates the linked price levels and FIFO queues, and
collects at most three makers. Every step records the taker quantity before its
fill, the resting maker price, canonical maker ledger, and only the successor,
worse-level, or rent-recipient accounts needed by that transition. The market
page converts the plan into one `match_limit_order` instruction per maker and
sends every instruction in one Solana transaction.

The planner's RPC reads are advisory. Each instruction revalidates best-price,
FIFO, PDA, owner, crossing-price, and reciprocal-link invariants against the
state produced by the preceding instruction. A stale later step therefore
rolls back the entire transaction. The three-maker cap bounds transaction size,
account metadata, compute, and stale-state exposure. When the planned path
proves that no crossing maker remains, the page appends an insert-or-append
instruction for the valid remainder to the same transaction. A remainder stays
free and visible only when crossing liquidity remains beyond the cap or its bid
notional rounds to zero.

New routes should keep read-only structure server-rendered and move only wallet,
transaction, state, and browser-dependent behavior behind `"use client"`.

## Dependency note

Anchor `1.2.0` requires legacy `@solana/web3.js` v1, while the faucet uses the
latest compatible `@solana/spl-token` package for associated token accounts.
`npm audit --omit=dev` currently reports nine transitive advisories through
`bigint-buffer`, `jayson`, `stream-json`, `toml`, and `uuid`.

npm's proposed forced remediations replace the direct packages with incompatible
historical or invalid versions (`@solana/spl-token@0.1.8`,
`@solana/web3.js@0.0.3`, or `@anchor-lang/core@0.0.0`), so they are not applied.
This boundary should be rechecked when the Solana/Anchor client stack publishes
compatible patched releases. Do not run `npm audit fix --force` without first
validating all generated transactions and wallet flows.
