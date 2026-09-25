# Crankless Matching and Settlement Design

## Status

Status: **In progress.** Pure settlement planning and atomic one-maker matching
are implemented for both partial and complete maker fills. Complete fills repair
FIFO and price-level links, advance the best price, and close an empty level.
Automatic bounded multi-maker traversal and remainder posting remain.

This document explains why Tidebook is targeting bounded crankless settlement,
how that differs from Phoenix and older OpenBook designs, and which accounts a
matching instruction must read and write. The first implementation slice proves
maker-price settlement, price-time priority, and safe queue removal.

## The problem in plain language

Suppose Alice has an ask to sell 2 BTC at 100 USDT and Bob submits a bid to buy
1 BTC at 100 USDT. A complete trade must make all of these changes together:

1. remove 1 BTC from Alice's locked amount;
2. give Alice a claim to 100 USDT;
3. remove 100 USDT from Bob's locked amount;
4. give Bob a claim to 1 BTC;
5. reduce Alice's resting order from 2 BTC to 1 BTC;
6. record that Bob's incoming order is filled;
7. update the price level's aggregate quantity.

If only some changes commit, tokens or accounting can be lost. Tidebook must
therefore make the book mutation and balance mutation in one Solana transaction.
On failure, the entire transaction rolls back.

The difficult part is not the arithmetic. A Solana instruction must declare all
accounts that it may modify before execution. The program cannot find a maker
order during matching and then turn an undeclared maker account into a writable
account.

## What "settlement" means

Tidebook distinguishes two operations:

- **ledger settlement** changes who owns the claim to tokens already held in
  the market vaults;
- **withdrawal** transfers a trader's free claim from a market vault to the
  trader's SPL Token account.

Crankless atomic settlement means ledger settlement completes in the same
transaction as matching. It does not require every fill to transfer tokens to
both traders' wallet token accounts immediately.

This distinction keeps the matching account list bounded. A maker does not need
to be online, and the taker does not need to know the maker's preferred wallet
token accounts. The program credits the maker's canonical on-chain balance
record instead.

## Phoenix: centralized market state

Phoenix places the bids, asks, and registered traders' internal balances inside
one fixed-capacity market account. Its matching instruction can therefore:

```text
load one writable market
  -> find the best opposing orders
  -> update each maker's internal balance
  -> update the taker's balance or token transfer
  -> commit every change together
```

No later transaction is required to make a maker's exchange balance correct.
Phoenix may emit events, but those events are for observation and indexing; they
are not a queue of unfinished balance changes.

This state layout provides a small, predictable account list and synchronous
composability. Its costs are a large rent-funded market account, fixed capacity,
specialized in-account data structures, and a shared writable hotspot for all
activity in one market.

## Older OpenBook: distributed trader state and deferred work

Older OpenBook/Serum-style markets keep each trader's state in a separate
`OpenOrders` account. During matching, an incoming taker can cross orders owned
by makers whose `OpenOrders` accounts were not supplied to the instruction.

The architecture handles this by recording fills in an event queue:

```text
taker order matches
  -> fill event enters event queue
  -> a cranker later supplies maker OpenOrders accounts
  -> ConsumeEvents updates maker balances
  -> maker later withdraws with SettleFunds
```

This separates user state and allows capacity to be distributed across many
accounts, but correct and timely maker accounting depends on event consumption.
It also introduces queue capacity, crank incentives, monitoring, and recovery
as protocol concerns.

## Tidebook decision: bounded crankless PDA matching

Tidebook will keep its educational PDA-based order book instead of immediately
adopting Phoenix's fixed slab. It will add canonical per-trader balance PDAs and
require the client to supply a bounded matching path.

This is Phoenix-like in settlement semantics but not in storage layout:

| Concern | Phoenix | Tidebook target |
| --- | --- | --- |
| Book storage | Trees inside one market account | Price-level and order PDAs |
| Trader balances | Entries inside the market account | One PDA per market and trader |
| Matching accounts | Primarily the market and vault context | Client-supplied levels, orders, and maker balances |
| Settlement | Atomic internal ledger update | Atomic internal ledger update |
| Crank | Not required | Not required for completed matches |
| Capacity bound | Fixed market allocation | Per-instruction account and compute budget |

"Bounded" means one instruction processes at most a configured number of
resting orders. If a large taker order needs more work, the client submits
another transaction with the next matching path. Every successfully processed
chunk is final and settled; there is no event waiting for a cranker.

## Proposed TraderBalance PDA

Each `(market, trader)` pair has one deterministic account:

```text
TraderBalance = ["trader_balance", market, trader]
```

Proposed fields:

| Field | Meaning |
| --- | --- |
| `market` | Market whose vaults back this balance |
| `owner` | Wallet allowed to deposit, place orders, cancel, and withdraw |
| `base_free` | Base atoms available to withdraw or reuse |
| `base_locked` | Base atoms reserved by open asks |
| `quote_free` | Quote atoms available to withdraw or reuse |
| `quote_locked` | Quote atoms reserved by open bids |
| `bump` | Canonical PDA bump |

The balance account is an accounting claim. The actual SPL tokens remain in
the market's canonical base and quote vaults.

For each mint, this conservation relationship must hold across all traders:

```text
vault amount = sum(free balances) + sum(locked balances) + protocol fees
```

Fees are a future term and are zero until fee accounting is introduced.

## Order lifecycle under this model

### Deposit

The trader transfers SPL tokens into the canonical market vault. The same
instruction increases the corresponding free balance:

```text
wallet base account --10 base--> base vault
TraderBalance.base_free: 0 -> 10
```

The token transfer and balance credit must be atomic.

### Place a resting ask

An ask reserves base:

```text
base_free:   10 -> 8
base_locked:  0 -> 2
```

The order records 2 base atoms of remaining quantity. The price-level and FIFO
links are updated in the same instruction.

### Place a resting bid

A bid reserves its maximum quote notional:

```text
required quote = limit price * quantity / base scale
quote_free    -= required quote
quote_locked  += required quote
```

Checked integer arithmetic and the market's existing fixed-point price rules
remain mandatory.

### Match

For a trade of quantity `Q` at resting price `P`:

```text
quote paid = P * Q / base scale
```

If the resting order is an ask:

```text
maker.base_locked  -= Q
maker.quote_free   += quote paid
taker.quote_free   -= quote paid
taker.base_free    += Q
```

If the resting order is a bid, the assets move in the opposite direction:

```text
maker.quote_locked -= quote paid
maker.base_free    += Q
taker.base_free    -= Q
taker.quote_free   += quote paid
```

The order's remaining quantity, locked collateral, price-level aggregate, FIFO
links, market counters, and both traders' balances change atomically.

### Better execution for a taker

A bid may specify a limit of 105 but match a resting ask at 100. The taker owes
only 100 because the current bounded instruction debits free balance directly
at the maker price. It does not create or lock an incoming taker order first.
If the taker is larger than the maker, only the actual fill is debited and the
unprocessed remainder stays free for the client's next explicit instruction.

### Cancel

Cancellation removes the order from its FIFO queue and moves its remaining
collateral from locked back to free. It does not need an SPL Token transfer:

```text
ask: base_locked  -> base_free
bid: quote_locked -> quote_free
```

Cancellation remains allowed while the market is paused. Withdrawal is a
separate owner-authorized operation.

### Withdraw

The owner chooses an SPL Token account with the correct mint. The vault
authority PDA transfers at most the requested free amount and the program
decreases the corresponding free balance in the same instruction. Locked funds
can never be withdrawn.

## How a matching transaction is constructed

The off-chain client reads the program-maintained index, beginning at
`market.best_ask` for an incoming bid or `market.best_bid` for an incoming ask.
It collects up to the instruction's matching limit:

1. opposing price-level PDAs in price-priority order;
2. resting order PDAs in FIFO order;
3. canonical `TraderBalance` PDAs for the makers;
4. the taker's canonical `TraderBalance` PDA;
5. the market and canonical vault accounts required by the instruction.

These accounts are hints, not trusted truth. The on-chain program must verify:

- every PDA is derived canonically;
- each level belongs to the market and expected side;
- the first level is the market's current best opposing level;
- level links follow strict price priority;
- order links follow FIFO order;
- each order belongs to its level and is `Open`;
- each maker balance belongs to the order owner and market;
- prices cross the taker's limit;
- no order or level in the supplied path is skipped, duplicated, or
  substituted. One canonical maker balance may legitimately serve several
  orders owned by the same maker.

If the client built its path from stale state, validation fails and all changes
roll back. The client refreshes the market and retries. This is normal optimistic
concurrency, not a reason to trust the client.

## Matching algorithm boundary

For an incoming bid with limit price 105:

```text
start at best ask
while capacity remains:
  stop if best ask price > 105
  take the oldest order at that price
  fill min(incoming remaining, resting remaining)
  update balances and quantities
  remove a completely filled order
  remove an empty price level
  stop when incoming order is filled or supplied accounts are exhausted
```

An incoming ask is symmetric and stops when the best bid is below its limit.

The implemented instruction supplies one maker. If that capacity ends while the
incoming request still has quantity:

- the processed fills remain valid and atomically settled;
- the remainder stays in the taker's free internal balance;
- the web client retains the remainder in its form so the trader can match the
  next maker or post it after refreshing the book.

Automatic multi-maker traversal and atomic remainder posting are the next
policy milestone. The current behavior never silently drops quantity or leaves
collateral without a balance claim.

## Events and indexers

Tidebook may emit `OrderPlaced`, `Fill`, `OrderCanceled`, `Deposit`, and
`Withdrawal` events for history and UI indexing. Events do not authorize or
complete a balance transition. If an indexer is offline, the on-chain order book
and trader balances remain correct and withdrawals remain possible.

This is the defining difference from an event queue containing deferred
settlement work.

## Failure and safety rules

1. Book state and balance state must never be committed separately.
2. A fill can consume only collateral already recorded as locked.
3. Free and locked balances use checked arithmetic.
4. Only the balance owner can withdraw or intentionally place/cancel orders.
5. A maker need not sign a match; its prior open order is standing consent.
6. The program derives every maker balance from the resting order's owner.
7. Vault amounts must continue to back all recorded claims.
8. A filled order has zero remaining quantity and zero locked collateral.
9. A partially filled order remains in its original FIFO position.
10. A canceled order cannot later be matched.
11. Pausing blocks new placement and matching but not cancellation or withdrawal.
12. Events are never used as unfinished settlement state.

## Tradeoffs accepted by Tidebook

### Benefits

- no external cranker is required for maker balances to become correct;
- every processed fill is final when its transaction confirms;
- makers can remain offline;
- PDA state is easier to inspect and teach than a custom zero-copy slab;
- matching work has an explicit per-transaction bound.

### Costs

- the client must discover and provide the matching path;
- more account metadata is needed than in Phoenix;
- stale clients may need to refresh and retry;
- transaction account and compute limits bound matches per instruction;
- linked PDA mutation requires extensive reciprocal-link validation;
- writable market and vault accounts remain contention points;
- rent grows with traders, orders, and active price levels.

This is a research-oriented correctness design. If benchmarks later show that
PDA and account-list overhead dominate, a versioned future market can adopt a
Phoenix-style fixed-capacity slab without pretending the two layouts are
compatible in place.

## Implementation sequence

The safe order is:

1. ~~Finish and test price-level/FIFO maintenance without matching.~~
2. ~~specify and add `TraderBalance` accounts;~~
3. ~~add atomic deposits and withdrawals;~~
4. ~~move placement and cancellation collateral accounting through free/locked
   balances;~~
5. ~~Define and implement the one-maker matching account contract and explicit
   free-balance remainder policy.~~
6. ~~Add partial- and full-maker tests before multi-fill behavior.~~
7. **In progress:** add bounded multi-maker, multi-level, stale-path, rollback,
   and broader conservation tests.
8. Emit informational events; the one-maker web client is implemented.
9. benchmark account count, compute use, contention, and retry rate before
   considering a slab-based redesign.

Every future expansion must preserve the existing atomic balance and book
invariants under success, stale-account failure, and arithmetic failure.

## Primary implementation references

- [Phoenix repository](https://github.com/Ellipsis-Labs/phoenix-v1)
- [Phoenix FIFO market](https://github.com/Ellipsis-Labs/phoenix-v1/blob/master/src/state/markets/fifo.rs)
- [Phoenix new-order processing](https://github.com/Ellipsis-Labs/phoenix-v1/blob/master/src/program/processor/new_order.rs)
- [Phoenix trader balances](https://github.com/Ellipsis-Labs/phoenix-v1/blob/master/src/state/trader_state.rs)
- [OpenBook instruction model](https://github.com/openbook-dex/program/blob/master/dex/src/instruction.rs)
- [OpenBook accounts and event consumption](https://github.com/openbook-dex/program/blob/master/dex/src/state.rs)
