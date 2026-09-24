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
- Create markets as an active protocol administrator.
- Place collateralized bid and ask limit orders from owned token accounts,
  including empty, best, middle, worst, and same-price FIFO insertion paths.
- List wallet-owned orders and cancel them, including while a market is paused.
- Pause, unpause, and safely close markets as their authority.
- Manage protocol administrators as the super-admin.

The checked-in client IDL mirrors the generated Anchor IDL. After any account
layout or instruction change, rebuild and redeploy the program before using the
updated client against devnet.

Override the default public devnet RPC endpoint locally with:

```bash
NEXT_PUBLIC_SOLANA_RPC_URL=https://your-devnet-endpoint.example
```

Store that value in `app/.env.local`; the file is ignored by Git.

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

New routes should keep read-only structure server-rendered and move only wallet,
transaction, state, and browser-dependent behavior behind `"use client"`.

## Dependency note

Anchor `1.2.0` requires legacy `@solana/web3.js` v1. npm currently reports
moderate transitive advisories through `jayson`, `stream-json`, and `uuid` in
the latest v1 release. npm's proposed automatic remediation downgrades web3.js
to an incompatible historical version, so it is not applied. This boundary
should be rechecked when Anchor supports the newer Solana JavaScript client or
web3.js v1 publishes a patched dependency graph.
