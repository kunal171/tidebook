# Price-Level Index and FIFO Queue Design

## Status and scope

Status: **Foundation implemented; sorted levels and indexed removal remain in
progress**.

This milestone organizes resting orders by price and arrival time. It does not
match orders, partially fill them, or settle trades. Placement and cancellation
must keep the index correct before matching logic is allowed to depend on it.

The following matching milestone will use the bounded crankless model described
in [`crankless-settlement-design.md`](crankless-settlement-design.md). That later
instruction will require clients to supply the relevant levels, FIFO orders,
and maker balance PDAs. Consequently, every link established here is a security
boundary and must be validated rather than treated as trusted client input.

## Required ordering

Tidebook uses price-time priority:

1. bids with higher prices have priority over lower bids;
2. asks with lower prices have priority over higher asks;
3. orders at the same price are processed first in, first out (FIFO).

For example, if bid orders A and B arrive at price 100, followed by C at 99,
the priority order is A, B, C. A later order at 101 moves ahead of all three
because price priority is evaluated before arrival time.

## Chosen account model

Each active `(market, side, price)` combination has a deterministic
`PriceLevel` PDA:

```text
PriceLevel = ["price_level", market, side_seed, price.to_le_bytes()]
```

`side_seed` must use explicit stable bytes (`"bid"` or `"ask"`), rather than a
Rust enum's in-memory discriminant. This keeps PDA derivation stable across the
Rust program, tests, and TypeScript client.

Implemented `PriceLevel` fields:

| Field | Meaning |
| --- | --- |
| `market` | Market that owns the level |
| `side` | Bid or ask |
| `price` | Tick-aligned raw price shared by every queued order |
| `better_price` | Adjacent price with higher matching priority |
| `worse_price` | Adjacent price with lower matching priority |
| `first_order` | Oldest open order at this price |
| `last_order` | Newest open order at this price |
| `total_remaining_quantity` | Sum of remaining base quantity at this level |
| `order_count` | Number of open queued orders |
| `rent_payer` | Recipient of rent when the empty level closes |
| `bump` | Canonical PDA bump |

`better_price` and `worse_price` are preferred over `previous` and `next`
because bid and ask prices sort in opposite numeric directions.

The existing `Order` account includes:

| Field | Meaning |
| --- | --- |
| `price_level` | Canonical level containing the order |
| `previous_order` | Older order at the same price, if one exists |
| `next_order` | Newer order at the same price, if one exists |

The market's existing `best_bid` and `best_ask` fields become the entry points
to the two sorted level lists. A best level always has `better_price = None`.

## Queue mutations

### Place at an existing level

The client supplies the canonical price level and its current tail order. The
program validates the tail against `price_level.last_order`, links the old tail
to the new order, sets the new order's `previous_order`, and updates the level's
quantity and count. All collateral and queue mutations remain in one atomic
instruction.

This path is implemented by `append_limit_order`. Integration tests cover bid
and ask append, a three-order FIFO chain, stale-tail and opposite-side level
rejection, paused-market rejection, insufficient collateral, and atomic
preservation of links, aggregates, counters, order accounts, and vault balances
after failure.

### Place at a new level

`insert_limit_order` owns both first-level creation and sorted new-level
insertion. For an empty side, the client supplies no neighbors. New-best insertion supplies
only the canonical current best as the worse neighbor. Middle insertion supplies
two canonical levels whose reciprocal links prove adjacency. New-worst insertion
supplies only the terminal better level, whose worse link must be empty. Bid
prices must strictly descend and ask prices must strictly ascend across every
link. The instruction rewires the supplied neighbors atomically with collateral
transfer, level creation, order creation, and market counters.

Creating the level separately from the order is rejected because it permits an
empty active level if the later order transaction never succeeds.

### Cancel an order

The client supplies the order's optional older and newer neighbors. The program
validates reciprocal links before unlinking the order. Head, middle, tail, and
only-order removal are distinct cases and require dedicated tests.

If the final order leaves a level, the client also supplies its adjacent price
levels and stored `rent_payer`. The program unlinks and closes the empty level,
updates `best_bid` or `best_ask` when necessary, and refunds the level's rent.
Collateral refund and index mutation succeed or roll back together.

## Security invariants

1. A level PDA is derived from its stored market, side, and price.
2. Every queued order belongs to the same market, side, and price as its level.
3. A non-empty level has both `first_order` and `last_order`; an empty level is
   closed before the instruction completes.
4. The first order has no older neighbor and the last order has no newer
   neighbor.
5. Every supplied adjacent order or level must link back to the account being
   changed.
6. Level prices are strictly ordered with no duplicate PDA for the same
   `(market, side, price)` tuple.
7. `order_count` changes by exactly one on placement or cancellation.
8. `total_remaining_quantity` uses checked arithmetic and equals the aggregate
   quantity represented by successful mutations.
9. `best_bid` points to the highest non-empty bid level; `best_ask` points to the
   lowest non-empty ask level.
10. The level index, order queue, collateral movement, and market counters are
    updated atomically.

## Solana account-list boundary

An on-chain program cannot discover arbitrary PDAs or scan all orders by
itself. The client therefore supplies neighboring accounts. Those accounts are
untrusted hints: the program must derive canonical addresses and validate
prices and reciprocal links before mutation.

Optional neighbor accounts reduce the number of instruction variants but make
the account contract more complex. If Anchor optional-account ergonomics make
the generated clients unclear, explicit instruction variants are preferable to
loosely typed `remaining_accounts`.

The current web client locates a missing price level by walking from the
market best price through canonical `worse_price` links. It rejects cycles,
cross-market or cross-side links, PDA/price mismatches, broken reciprocal links,
and non-strict ordering before constructing the instruction. This O(levels)
RPC walk improves diagnostics but is not a security boundary: the program
repeats all canonical-address and neighbor checks atomically because another
transaction may mutate the book after the client reads it. A later indexer may
provide the same better/worse accounts more efficiently without changing the
instruction contract.

## Tradeoffs and rejected alternatives

| Approach | Benefits | Costs and reason for decision |
| --- | --- | --- |
| Growing `Vec` in `Market` | Simple lookup and serialization | Requires bounded capacity or reallocations, makes one large hot account, and increases write contention; rejected |
| One PDA per price level with linked levels | Deterministic, independently sized, best price available in O(1) | Costs rent per active price and requires clients to supply validated neighbors; chosen |
| Doubly linked orders | O(1) append and removal when neighbors are supplied | Mutates adjacent accounts and expands cancellation account lists; chosen because owner cancellation must not require scanning |
| Singly linked orders | Smaller order state | Removing a middle or tail order requires traversal or a trusted predecessor; rejected |
| Keep empty levels for reuse | Avoids repeated allocation | Empty-level accumulation enables rent/state griefing and makes finding the next non-empty best price unbounded; rejected |
| Separate level-creation transaction | Simpler placement branches | Can leave an empty indexed level and introduces an avoidable race between transactions; rejected |
| Fixed slab or crit-bit tree | Compact, fast structure used by mature order books | Considerably more complex memory management and fixed-capacity planning; deferred until measurements justify it |

The chosen PDA layout is not presented as more efficient than a Phoenix-style
in-account tree. It is selected because each state transition is inspectable and
testable during this research phase. Its account-list, rent, and contention
costs must be measured before treating it as a production architecture.

## Concurrency and cost

Orders at different existing price levels can mutate separate level accounts,
but the current market account remains writable for `next_order_id` and
`open_order_count`. The market is therefore still a global write-contention
point. Removing that bottleneck would require a later order-ID and accounting
redesign and is outside this milestone.

New-level insertion and final-level removal also lock adjacent level accounts.
This is a deliberate cost of maintaining a trustless sorted index without an
unbounded central account.

The trader pays rent when a new price level is created. The level records that
wallet as `rent_payer`; its lamports return to the same address when the final
order removes and closes the level. Order-account rent reclamation remains a
separate future decision.

## Required test matrix

Current coverage:

- first bid and first ask create canonical best levels;
- an empty bid or ask side accepts no-neighbor insertion and rejects an unexpected neighbor;
- omitting the current-best neighbor on a non-empty side is rejected without
  changing pointers, counters, or accounts;
- better, middle, and worse bid and ask levels preserve strict ordering and
  reciprocal links;
- malformed, stale, cross-market, cross-side, and noncanonical neighbor hints
  roll back without changing links, counters, orders, or vault balances;
- a second bid at the same price appends behind the FIFO tail and updates level
  aggregates atomically;
- bid and ask append paths preserve the same queue invariants;
- three bids preserve reciprocal `A <-> B <-> C` FIFO links;
- stale tails, opposite-side levels, paused markets, and insufficient collateral
  are rejected without mutating state or vault balances.

Remaining tests required before matching:

- head, middle, tail, and only-order cancellation repair reciprocal links;
- level count and aggregate quantity update with checked arithmetic;
- final cancellation closes the level and returns rent;
- removing the best level advances the appropriate market pointer;
- removing a middle level repairs both adjacent levels;
- wrong level PDA, side, price, tail, order neighbor, level neighbor, or rent
  recipient is rejected atomically;
- failed collateral deposit or refund leaves every queue and level unchanged;
- paused markets reject placement but still allow indexed cancellation;
- market shutdown remains impossible while any indexed order is open.

## Implementation sequence

1. ~~Add stable price-level seed helpers and account layouts.~~
2. ~~Add derivation and serialization tests.~~
3. ~~Create the first bid/ask level and append a second same-price bid.~~
4. ~~Complete same-price FIFO failure paths and ask-side coverage.~~
5. ~~Add sorted better, middle, and worse level insertion.~~
6. Extend cancellation for order unlinking.
7. Close and unlink empty levels, including best-price updates.
8. ~~Update the checked-in IDL and web client for sorted insertion.~~
9. Update the architecture diagram and invariant list after indexed
   cancellation is complete.

Matching starts only after this matrix passes and the index can be treated as a
trusted program-maintained structure. "Trusted" here means maintained by the
program; a matching instruction must still revalidate the supplied traversal
path because clients may submit stale or malicious accounts.
