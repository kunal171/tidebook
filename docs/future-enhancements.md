# Future Enhancements

## Purpose

This document records Tidebook's planned evolution beyond the current
fee-aware central limit order book. It is a roadmap, not a description of
implemented behavior. Each phase should be developed on a separate feature
branch and merged only after its account model, failure paths, frontend, and
documentation are complete.

The target is a hybrid market with four distinct responsibilities:

```text
External price feeds
        |
        v
Reference-price service ---> Offchain market maker ---> Tidebook CLOB
        |                                                   |
        v                                                   v
Monitoring and circuit breakers <--- Smart order router ---> Tidebook AMM
```

An oracle reports an external reference price. It does not create liquidity or
force Tidebook's executable price to follow another venue. The market maker
creates CLOB liquidity around that reference, while arbitrage aligns executable
prices. The optional AMM supplies backstop liquidity when the order book is
thin.

## Design principles

1. The oracle is a reference and safety input, not the execution-price engine.
2. Existing limit orders continue to execute at the resting maker price.
3. CLOB custody, AMM reserves, and protocol fees remain separately auditable.
4. Every route has an explicit maximum spend or minimum receive.
5. A stale or uncertain oracle must fail safely and visibly.
6. Offchain discovery and quoting are untrusted; onchain instructions validate
   every account and state transition.
7. Hybrid execution must remain atomic: a failed later leg rolls back earlier
   CLOB fills and AMM swaps.
8. Architecture changes are chosen from measurements, not assumptions.

## Phase 0: clean devnet baseline

Deploy the current fee-accounting program with fresh state before introducing
oracle or AMM accounts. The research project currently favors a clean redeploy
over migration of incompatible development accounts.

Deliverables:

- initialize protocol governance and a fresh BTC/USDT research market;
- execute deposit, placement, matching, cancellation, fee accrual, fee
  withdrawal, and safe market closure;
- record addresses and transactions in `devnet-addresses.md`;
- capture compute units, transaction size, account count, and RPC latency for
  the current CLOB baseline.

Done when the existing fee-aware CLOB completes its full lifecycle on devnet.

## Phase 1: oracle compatibility and account design

Proposed branch: `feature/oracle-foundation`.

Pyth is the initial provider candidate. Before adding it to production
instructions, verify its current Solana receiver SDK against Tidebook's Anchor,
Solana SDK, and LiteSVM dependency graph. The provider must stay behind a small
Tidebook-owned interface so a dependency or provider change does not spread
through the matching engine.

Proposed account:

```text
MarketOracle = ["market_oracle", market]
```

Candidate state:

| Field | Purpose |
| --- | --- |
| `market` | Canonical Tidebook market |
| `provider` | Validated oracle-provider variant |
| `base_feed_id` | Base/USD feed identifier |
| `quote_feed_id` | Quote/USD feed identifier |
| `max_age_seconds` | Maximum accepted update age |
| `max_confidence_bps` | Maximum confidence interval relative to price |
| `max_deviation_bps` | Optional safety threshold for monitoring or guards |
| `enabled` | Governance-controlled integration state |
| `bump` | Canonical PDA bump |

For BTC/USDT, the reference price should be composed as:

```text
BTC/USDT = BTC/USD / USDT/USD
```

The implementation must normalize signed oracle exponents with checked
integer arithmetic. It must not assume that USDT is always exactly one USD.

Required tests:

- correct feed and decimal normalization;
- stale update rejection;
- wrong feed ID or oracle program owner;
- zero or negative price;
- excessive confidence interval;
- exponent, multiplication, and division boundaries;
- deterministic mock oracle accounts under LiteSVM.

Done when a pure oracle adapter and its failure paths are fully tested without
changing order execution.

## Phase 2: reference-price UI and market health

Expose reference data on each market page:

- oracle price and update age;
- confidence interval;
- best bid and best ask;
- CLOB midpoint;
- last traded price from confirmed fill events;
- midpoint-to-oracle and last-trade-to-oracle deviation.

These values must retain distinct names. "Market price" is ambiguous because
the external reference, midpoint, and most recent Tidebook execution can differ.

The first release should use oracle data for display and alerts only. A hard
onchain price band is deferred until devnet data shows appropriate freshness
and deviation thresholds. A hard band protects against extreme execution but
can also halt valid price discovery when an oracle is stale.

Done when users can understand both external reference conditions and actual
Tidebook liquidity without changing matching semantics.

## Phase 3: reference market-maker service

Proposed branch: `feature/reference-market-maker`.

Build a separate TypeScript service under `services/market-maker/`. A Solana
program cannot react to price changes by itself; an offchain actor must observe
prices and submit transactions.

The service should:

1. consume oracle updates;
2. read Tidebook levels and its own open orders;
3. calculate inventory-adjusted bid and ask quotes;
4. cancel stale quotes and submit replacements;
5. maintain multiple bounded price levels;
6. persist transaction state and recover after restart;
7. stop quoting when its oracle, RPC, inventory, or transaction health limits
   are violated.

Mandatory controls:

- dedicated limited-funds signer;
- maximum base inventory and quote exposure;
- maximum order size and number of live orders;
- oracle freshness and confidence limits;
- maximum CLOB/oracle deviation;
- self-trade prevention;
- retry and priority-fee limits;
- heartbeat, alerting, and manual kill switch.

Efficient quoting may justify bounded `cancel_orders`, `replace_order`, or
`cancel_all_orders` instructions. These should be introduced only after the
single-order service exposes a measured transaction bottleneck.

Done when the bot maintains two-sided devnet liquidity, safely cancels stale
quotes, and recovers without duplicating orders after restart.

## Phase 4: monitoring and circuit breakers

Add an event consumer or indexer for fills, fees, oracle observations, spreads,
inventory, transaction failures, and retries. Dashboards should distinguish
protocol state from derived analytics.

Initial circuit breakers belong in the market-maker service:

- stop quoting on stale or uncertain oracle data;
- stop quoting when deviation exceeds a configured threshold;
- stop quoting when inventory or loss limits are reached;
- require deliberate operator recovery after a severe fault.

Onchain circuit breakers may be added later for protocol-wide protection, but
they must preserve cancellation and withdrawal exits while trading is halted.

## Phase 5: standalone AMM design

Proposed design branch: `design/hybrid-amm`.

The first AMM should use a constant-product curve:

```text
base_reserve * quote_reserve = k
spot_price = quote_reserve / base_reserve
```

The oracle may validate the initial reserve ratio and detect dangerous
deviation. It must not rewrite reserves or guarantee the AMM price; trades and
arbitrage move the pool price.

Proposed accounts:

```text
AmmPool       = ["amm_pool", market]
AmmAuthority  = ["amm_authority", amm_pool]
AmmBaseVault  = ["amm_vault", amm_pool, base_mint]
AmmQuoteVault = ["amm_vault", amm_pool, quote_mint]
LpPosition    = ["lp_position", amm_pool, provider]
```

AMM vaults must not be shared with CLOB vaults. Separate custody preserves
simple conservation equations and limits the blast radius of accounting bugs.

The design must specify:

- initial and subsequent LP-share calculation;
- minimum locked liquidity;
- exact-input swap formulas and rounding direction;
- LP fee and protocol-fee allocation;
- slippage and deadline checks;
- checked `u128` intermediate arithmetic;
- pause, emergency exit, and safe closure;
- Token Program and future Token-2022 boundaries;
- oracle validation during initialization.

Done when the formulas, invariants, attacks, account sizes, and test matrix are
documented and exercised by pure Rust property tests.

## Phase 6: standalone AMM implementation

Suggested feature sequence:

1. `feature/amm-pool-foundation`
2. `feature/amm-liquidity`
3. `feature/amm-swaps`
4. `feature/amm-fees`

Expected instructions:

```text
initialize_amm_pool
add_liquidity
remove_liquidity
swap_exact_input
pause_amm_pool
unpause_amm_pool
collect_amm_protocol_fees
close_amm_pool
```

Integration tests must cover proportional liquidity, both swap directions,
rounding, invariant preservation, fees, slippage, incorrect accounts, paused
behavior, arithmetic limits, token CPI failure, and global asset conservation.

Done when the AMM operates safely as an independent devnet venue before any
hybrid route is introduced.

## Phase 7: hybrid CLOB and AMM routing

Implement an offchain smart-order router before coupling AMM logic directly to
`match_limit_order`. The router compares:

- executable CLOB prices and quantities;
- AMM marginal price and price impact;
- taker, swap, and protocol fees;
- transaction account and compute limits.

A route may atomically sequence:

```text
match best CLOB maker
match next CLOB maker
swap the safe remainder through the AMM
```

Required guarantees:

- no execution violates the user's limit price;
- exact maximum spend or minimum receive is enforced;
- quote simulation and execution use identical rounding;
- a remainder is never silently discarded;
- failure of any leg rolls back the entire route;
- CLOB and AMM fees remain separately attributable;
- stale CLOB or reserve state fails safely and can be replanned.

Only benchmarks should decide whether a later combined onchain routing
instruction is worth its larger account contract and audit surface.

## Phase 8: benchmarking and hardening

Measure each component independently and in hybrid transactions:

- compute units and loaded-account data;
- serialized transaction size;
- signer, readonly, and writable account counts;
- maximum safe CLOB fills plus an AMM swap;
- oracle update and verification overhead;
- Address Lookup Table benefit;
- RPC traversal and confirmation latency;
- stale-state retry rate;
- writable-account contention;
- market-maker cancellation latency.

Fuzz and property-test arithmetic and conservation rules. Before mainnet,
complete threat modeling, operational runbooks, key separation, monitoring,
incident response, and an independent security audit.

## Estimated research effort

| Milestone | Focused effort |
| --- | ---: |
| Oracle compatibility and design | 8-16 hours |
| Oracle accounts, validation, tests, and UI | 16-24 hours |
| Reference market-maker service | 24-40 hours |
| Monitoring and price model | 8-16 hours |
| AMM design and mathematical tests | 12-20 hours |
| AMM liquidity, swaps, and fees | 24-40 hours |
| Hybrid router | 16-32 hours |
| Benchmarking and hardening | 12-24 hours |

The complete research prototype is approximately 100-180 focused hours. This
estimate does not include production operations, an independent audit, or
mainnet liquidity acquisition.

## Recommended next milestone

Begin `feature/oracle-foundation` with only:

1. the dependency compatibility spike;
2. a finalized oracle account and trust model;
3. checked price-normalization functions;
4. LiteSVM oracle fixtures and failure-path tests;
5. no changes to matching or settlement.

This creates the smallest safe foundation shared by the reference-price UI,
market-maker service, circuit breakers, and later AMM.

## References

- [Pyth real-time data on Solana](https://docs.pyth.network/price-feeds/core/use-real-time-data/pull-integration/solana)
- [Pyth price feeds and feed identifiers](https://docs.pyth.network/price-feeds/core/price-feeds)
- [Solana program model](https://solana.com/docs/core/programs)
- [Solana program execution](https://solana.com/docs/core/programs/program-execution)
- [Solana markets and atomic composability](https://solana.com/docs/defi)
