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
- Check whether the configured Tidebook program account is executable.
- Link to the program account in Solana Explorer.
- Present the current backend milestone and test status.

The program is deployed to devnet. Market creation remains intentionally
disabled until the generated Anchor IDL is wired into the client.

Override the default public devnet RPC endpoint locally with:

```bash
NEXT_PUBLIC_SOLANA_RPC_URL=https://your-devnet-endpoint.example
```

Store that value in `app/.env.local`; the file is ignored by Git.

## Application boundaries

- `app/layout.tsx` and route pages are Server Components by default.
- `components/solana-provider.tsx` owns the client-only wallet and RPC context.
- `components/dashboard.tsx` owns interactive wallet and program-status state.
- `lib/solana.ts` is the shared network and program-identity configuration.

New routes should keep read-only structure server-rendered and move only wallet,
transaction, state, and browser-dependent behavior behind `"use client"`.

## Dependency note

Anchor `1.2.0` requires legacy `@solana/web3.js` v1. npm currently reports
moderate transitive advisories through `jayson`, `stream-json`, and `uuid` in
the latest v1 release. npm's proposed automatic remediation downgrades web3.js
to an incompatible historical version, so it is not applied. This boundary
should be rechecked when Anchor supports the newer Solana JavaScript client or
web3.js v1 publishes a patched dependency graph.
