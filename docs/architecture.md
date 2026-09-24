# Tidebook Architecture

## 1. Purpose and current scope

`tidebook` is an Anchor program being developed as a research study of an
on-chain central limit order book.

The current implementation establishes the account and authorization foundation:

- initialize protocol governance from the program upgrade authority;
- manage deterministic, independently removable administrator records;
- create one deterministic market for a distinct SPL base/quote mint pair;
- pause, unpause, and close a market under authority control;
- create deterministic limit-order accounts while atomically locking collateral;
- create canonical price levels at the best, middle, or worst position on each
  side, append same-price orders behind a validated FIFO tail, and unlink
  canceled orders and empty levels atomically;
- validate behavior with in-process LiteSVM integration tests.

The program does **not** yet match opposing orders, partially fill them, or
settle trades. Orders hold collateral in market vaults, and canceling an open
order refunds its remaining locked amount while repairing the FIFO queue and
sorted level index. Final-order cancellation unlinks and closes the empty level
before the transaction completes.

## 2. System context

The program has three external roles:

| Role | Current capabilities |
| --- | --- |
| Super-admin | Add, disable, enable, and remove administrator records |
| Market authority | Initialize a market, pause it, unpause it, and close it while paused |
| Trader | Place a bid or ask limit order while the market is active |

The program reads SPL Token mints during market initialization and invokes the
Token Program when placing an order. Ask orders deposit base tokens; bid orders
deposit their calculated quote notional.

### Companion web application

The `app/` workspace is a Next.js App Router, React, and TypeScript client. It
connects browser wallets to devnet and uses a checked-in copy of the generated
Anchor IDL at `app/idl/tidebook.json`.

The shared role provider reads the protocol config and the connected wallet's
admin-record PDA. The header shows a `Super admin` or `Admin` badge only for an
active on-chain record. Role-aware routes are:

| Route | UI access | Purpose |
| --- | --- | --- |
| `/markets` | Public | Discover active and paused on-chain markets without connecting a wallet |
| `/markets/[address]` | Public viewing; connected wallet for trading; market authority for lifecycle controls | Inspect one market, place collateralized orders while active, and pause, unpause, or safely close an authorized market |
| `/admin` | Super-admin | Initialize governance and add, disable, enable, or remove admins |
| `/markets/new` | Active admin or super-admin | Create a market for two SPL mints |
| `/orders` | Any connected wallet | List wallet-owned orders and cancel orders whose status is `Open` |

The order form discovers a wallet-owned token account for the required mint and
passes the canonical vault accounts to the program. When the target price level
already exists, the client appends behind its FIFO tail. Otherwise, it walks
from the market's best price through `worse_price` links to find the exact
better/worse insertion gap. The walk checks for cycles, broken reciprocal links,
wrong markets or sides, and invalid price ordering before submitting.

This RPC traversal is only an advisory transaction-building step. The linked
book can change between reads and confirmation, so the on-chain instruction
derives every PDA and revalidates the neighboring prices and reciprocal links
atomically. A stale client transaction fails safely and can be retried after a
refresh. The browser walk is O(number of price levels); a production indexer
can replace it later without changing the program's neighbor-account contract.

The orders page filters program accounts by owner, displays locked collateral,
and refreshes after confirmation. For cancellation it fetches the live level,
supplies the order FIFO neighbors, and, only for a final-order removal, derives
the better/worse level PDAs and supplies the recorded rent payer. These accounts
are transaction-building hints; the program revalidates every link atomically.

Route visibility is a user-interface concern, not an authorization boundary.
Every privileged action must also be constrained by the Anchor program because
any client can submit an instruction directly.

The root layout and route pages are Server Components. Wallet Adapter, RPC
context, browser state, and future transaction signing are isolated behind
client-component boundaries. Planned routes can therefore share layouts and
loading/error boundaries without forcing the entire application into the client
bundle.

The client keeps network and program identity in `app/lib/solana.ts`.
The program address must remain synchronized with `declare_id!`, `Anchor.toml`,
and the deployment keypair.

The editable system diagram is available at
[`diagrams/tidebook-architecture.drawio`](diagrams/tidebook-architecture.drawio).

## 3. On-chain account model

### Protocol config PDA

```text
seeds = ["protocol_config"]
```

The singleton config stores the immutable super-admin authority selected during
protocol initialization. Initialization verifies that the signer is the
program's current upgrade authority.

### Admin record PDA

```text
seeds = ["admin", authority]
```

Each administrator has an independent account containing its authority,
provenance, status, and bump. This avoids an unbounded vector in one account and
allows records to be queried or closed independently.

### Market PDA

```text
seeds = ["market", base_mint, quote_mint]
```

There can be at most one market PDA for an ordered base/quote pair under this
program ID. Reversing the pair produces a different address.

| Field | Meaning |
| --- | --- |
| `authority` | Signer allowed to manage the market lifecycle |
| `base_mint` | Validated SPL Token mint used as the traded asset |
| `quote_mint` | Validated, distinct SPL Token mint used to price the base asset |
| `status` | `Active` or `Paused` |
| `next_order_id` | Monotonic identifier assigned to the next order; begins at `1` |
| `open_order_count` | Number of orders still eligible for matching or cancellation |
| `base_decimals` | Decimal precision read from the base SPL mint during initialization |
| `quote_decimals` | Decimal precision read from the quote SPL mint during initialization |
| `price_tick_size` | Smallest permitted price increment, expressed in raw price units |
| `quantity_lot_size` | Smallest permitted quantity increment, expressed in base-mint atoms |
| `best_bid` | Price of the first bid level, or `None`; sorted advancement is not implemented |
| `best_ask` | Price of the first ask level, or `None`; sorted advancement is not implemented |
| `bump` | Canonical market PDA bump |

### Market vault topology

Each market is created with one PDA authority and two canonical SPL Token
accounts. The authority PDA stores no account data; the program signs with its
seeds for collateral deposits, cancellation refunds, and vault closure.

```text
vault authority = ["vault-authority", market]
base vault      = ["vault", market, base_mint]
quote vault     = ["vault", market, quote_mint]
```

The base vault's token mint is the market's base mint, and the quote vault's
token mint is the market's quote mint. Both token accounts use the same vault
authority and begin with a zero balance. Market and vault creation occur in one
transaction, so a failed validation or account initialization leaves none of
them behind.

### Order PDA

```text
seeds = ["order", market, order_id.to_le_bytes()]
```

| Field | Meaning |
| --- | --- |
| `owner` | Trader that created the order |
| `market` | Market to which the order belongs |
| `order_id` | Market-local monotonic order number |
| `side` | `Bid` or `Ask` |
| `price` | Quote-mint atoms per one whole base token; must be nonzero and aligned to the market tick size |
| `price_level` | Canonical market/side/price level containing this order |
| `previous_order` | Older order at the same price, if present |
| `next_order` | Newer order at the same price, if present |
| `quantity` | Base-mint atoms; must be nonzero and aligned to the market lot size |
| `remaining_quantity` | Unfilled quantity; initially equals `quantity` |
| `locked_collateral` | Base atoms for asks or quote atoms for bids currently held in the market vault |
| `status` | Initially `Open`; the owner can transition it to `Canceled`; `Filled` is modeled but not transitioned yet |
| `bump` | Canonical order PDA bump |

Each successful order placement increments `Market.next_order_id`. The order PDA
therefore also provides insertion order that can later support FIFO priority.

### Price-level PDA

```text
seeds = ["price_level", market, side_seed, price.to_le_bytes()]
```

A level stores its market, side, price, optional better/worse price links,
FIFO head and tail orders, aggregate remaining quantity, order count, rent
payer, and canonical bump. Insertion splices a new level at the empty, best,
middle, or worst position. Append validates the current tail before extending
the same-price FIFO queue. Cancellation removes an order in O(1) from supplied
reciprocal neighbors; when it removes the final order, the program repairs the
level list, advances the market best pointer when required, returns rent to the
stored payer, and closes the level.

## 4. Instruction architecture

| Instruction | Required signer | Important checks | State transition |
| --- | --- | --- | --- |
| `initialize_protocol` | Program upgrade authority | Program-data relationship and upgrade authority match; singleton PDAs are canonical | Creates protocol config and an active deployer admin record |
| `add_admin` | Super-admin | Signer matches config; target is valid; admin PDA is canonical | Creates an active admin record |
| `disable_admin` | Super-admin | Signer matches config; target is active and is not the super-admin | `Active -> Disabled` |
| `enable_admin` | Super-admin | Signer matches config; target is disabled | `Disabled -> Active` |
| `remove_admin` | Super-admin | Signer matches config; target is disabled | Closes the admin record |
| `initialize_market` | Active admin | Admin record belongs to signer, is canonical and active; both accounts deserialize as SPL mints; base and quote differ; tick and lot sizes are nonzero; market, vault authority, and vault PDAs are canonical | Atomically creates an active market and empty base/quote SPL Token vaults |
| `pause_market` | Market authority | `has_one = authority`; market is active | `Active -> Paused` |
| `unpause_market` | Market authority | `has_one = authority`; market is paused | `Paused -> Active` |
| `close_market` | Market authority | Market is paused; `open_order_count` is zero; both canonical vaults are empty | Closes both vaults and the market atomically, returning their rent to the authority |
| `insert_limit_order` | Trader | Market is active; grid and collateral checks pass; supplied optional neighbors are canonical, correctly ordered, and reciprocal | Atomically transfers collateral, creates a new level and its first order, splices it at the empty, best, middle, or worst position, updates the best pointer when required, and increments counters |
| `append_limit_order` | Trader | Existing level is canonical for market/side/price; supplied previous order is its open tail with no successor; collateral validation matches placement | Atomically transfers collateral, creates an order, links it behind the tail, and updates level aggregates and market counters |
| `cancel_limit_order` | Order owner | Order, level, FIFO neighbors, and optional level neighbors are canonical and reciprocal; status is `Open`; refund account and vault match the side | Refunds collateral and unlinks the order; final removal repairs best/adjacent levels, closes the empty level to its stored rent payer, and transitions `Open -> Canceled` even while paused |

### Market initialization flow

```text
Active administrator
   |
   | initialize_market(base mint, quote mint, tick size, lot size)
   v
Anchor account validation
   |-- base account is an SPL Mint
   |-- quote account is an SPL Mint
   |-- base mint != quote mint
   |-- tick size > 0
   |-- lot size > 0
   |-- market PDA matches the ordered pair
   |-- vault authority matches ["vault-authority", market]
   |-- vaults match ["vault", market, mint]
   v
Create Market PDA as Active
   |
   |-- create empty base SPL Token vault
   |-- create empty quote SPL Token vault
   `-- set both token-account authorities to the vault-authority PDA
```

### New-level insertion flow

```text
Trader
   |
   | insert_limit_order(side, price, quantity)
   v
Validate active Market + nonzero values
   |-- empty side requires better = None and worse = None
   |-- new best: worse is the canonical current best
   |-- middle: better and worse link reciprocally
   |-- new worst: better has no worse successor
   |-- bid prices descend; ask prices ascend
   |-- price % price_tick_size == 0
   |-- quantity % quantity_lot_size == 0
   |-- checked quote notional > 0
   |-- ask selects base mint and quantity
   |-- bid selects quote mint and quote notional
   |-- trader owns the collateral token account
   |-- vault matches ["vault", market, collateral mint]
   |
   | derive ["order", market, next_order_id]
   v
Transfer collateral into the canonical market vault
   |
   v
Initialize PriceLevel PDA; splice both supplied neighbors
   |
   v
Create Order PDA as Open and record locked_collateral
   |
   v
Increment Market.next_order_id
```

This operation is atomic: if account creation or validation fails, the market
counter and order state are not committed.

### Existing-level FIFO append flow

```text
Trader
   |
   | append_limit_order(side, price, quantity)
   v
Validate canonical existing PriceLevel
   |-- level belongs to market, side, and price
   |-- supplied previous order equals level.last_order
   |-- previous order belongs to the level and is Open
   |-- previous order has no next_order
   |-- price, quantity, collateral mint, owner, and vault are valid
   v
Compute checked next quantities and counters
   |
   v
Transfer collateral into the canonical vault
   |
   |-- previous_order.next_order = new order
   |-- new_order.previous_order = previous order
   |-- price_level.last_order = new order
   |-- add level quantity and order count
   `-- increment market order ID and open-order count
```

The level's `first_order` and the market's best-price pointer remain unchanged.
Every transfer, link, aggregate, and counter succeeds or rolls back together.

### Order cancellation flow

```text
Order owner
   |
   | cancel_limit_order(order_id)
   v
Validate ownership + Open status + canonical level
   |-- supplied FIFO neighbors exactly match and link back
   |-- bid selects quote vault; ask selects base vault
   |-- refund destination belongs to the order owner
   v
Compute checked market and level aggregate decrements
   |
   |-- level remains non-empty: splice previous <-> next
   `-- final order: validate better/worse levels + rent payer
                     repair adjacent links and market best
                     close the empty level
   v
Vault-authority PDA refunds remaining collateral
   |
   v
Clear order links, quantity, and collateral; set Canceled
```

Cancellation deliberately has no active-market requirement, preserving the
owner's exit path while a market is paused. The refund, queue repair, aggregate
updates, best-pointer update, and conditional level closure are one transaction;
any failed validation or CPI rolls the complete transition back.

### Safe market shutdown flow

```text
Market authority
   |
   | close_market
   v
Validate paused Market
   |-- open_order_count == 0
   |-- base vault amount == 0
   |-- quote vault amount == 0
   |-- both vault addresses and authority are canonical
   v
Vault-authority PDA closes base and quote vaults
   |
   v
Anchor closes the Market account
```

All three accounts close atomically. A failed vault closure leaves the market
and both vaults intact.

### Fixed-point price model

Tidebook does not use floating-point values on-chain. An order price represents
quote-mint atoms per one whole base token, while quantity represents base-mint
atoms. Their human-readable forms are:

```text
human_price    = price / 10^quote_decimals
human_quantity = quantity / 10^base_decimals
```

The quote notional used for validation is calculated with checked `u128`
arithmetic:

```text
quote_notional_atoms = price * quantity / 10^base_decimals
```

Integer division rounds down. Orders that round below one quote-mint atom are
rejected. For a base mint with 9 decimals and quote mint with 6 decimals, a
price of `100_000_000` represents `100.000000` quote tokens per base token, and
a quantity of `5_000_000` represents `0.005000000` base tokens. Their notional
is `500_000` quote atoms, or `0.500000` quote tokens.

## 5. Current invariants

The program currently enforces:

1. A market is identified by its ordered pair of mint addresses.
2. Both market assets are initialized SPL Token mint accounts.
3. Base and quote mint addresses must be different.
4. Only the stored authority can pause, unpause, or close the market.
5. A market must be active to accept a new order.
6. A market must be paused before it can be closed.
7. Price and quantity must both be nonzero.
8. Market tick and lot sizes must both be nonzero.
9. Order prices must be exact multiples of the market tick size.
10. Order quantities must be exact multiples of the market lot size.
11. Checked order notional must be at least one quote-mint atom.
12. An order ID is allocated only by incrementing its market's counter.
13. Only the configured super-admin can manage administrator records.
14. The super-admin cannot disable its own admin record.
15. An administrator must be disabled before its record can be removed.
16. Only a signer with its canonical active admin record can initialize a market.
17. Only the stored order owner can cancel an order.
18. Only an `Open` order can transition to `Canceled`.
19. Every market is initialized atomically with its canonical base and quote vaults.
20. Each vault is bound to the correct market mint and the shared vault-authority PDA.
21. Newly initialized market vaults have zero token balances.
22. Ask orders lock their quantity in base-mint atoms.
23. Bid orders lock their checked quote notional in quote-mint atoms.
24. The collateral token account must belong to the trader and use the side's expected mint.
25. Collateral transfer, order creation, and counter increment succeed or roll back together.
26. Only an open order can refund collateral, preventing duplicate withdrawals.
27. Cancellation refunds to an owner-controlled account of the correct mint.
28. Refund transfer, collateral clearing, and cancellation status update are atomic.
29. Order placement increments and cancellation decrements `open_order_count`.
30. A market can close only while paused with no open orders or vault balances.
31. Successful shutdown closes both canonical vaults and the market atomically.
32. A first level is derived canonically from market, side, and price and is
    created atomically with its first order.
33. Empty-side insertion requires no neighbors; new-best insertion on a non-empty
    side requires the canonical previous best as its worse neighbor.
34. Middle insertion requires reciprocal adjacent neighbors and strict price
    ordering; new-worst insertion requires the terminal better level.
35. An existing-level append accepts only the level's open tail with no newer
    successor.
36. FIFO tail linking, level aggregates, collateral transfer, and market
    counters are updated atomically.
37. Cancellation accepts only the exact stored FIFO neighbors with reciprocal
    links to the removed order.
38. Cancellation decrements level quantity/count and market open-order count
    with checked arithmetic in the same transaction as the collateral refund.
39. A final-order cancellation accepts only canonical reciprocal level
    neighbors, repairs them, and closes the now-empty level to its rent payer.
40. Removing a best level advances `best_bid` or `best_ask` to its worse
    neighbor; removing the only level clears the corresponding pointer.
41. Empty price levels never remain in the active sorted index.

## 6. Known architectural gaps

These are planned features, not defects in the current research milestone:

- No matching engine or partial-fill transitions.
- No settlement or fee accounting.
- Canceled order accounts are retained as history and their rent is not yet
  reclaimed.

## 7. Test architecture

Integration tests run against LiteSVM in
`programs/tidebook/tests/admin_flow.rs`,
`programs/tidebook/tests/cancel_order.rs`, and
`programs/tidebook/tests/market_close.rs`, and
`programs/tidebook/tests/order_flow.rs`.

The test harness:

1. loads the compiled SBF program into an in-process VM;
2. funds a generated payer;
3. inserts rent-exempt SPL Mint fixtures with valid packed mint state;
4. constructs Anchor instructions and versioned transactions;
5. sends transactions through LiteSVM;
6. deserializes resulting Anchor accounts and checks state.

The 87-test suite currently covers:

- upgrade-authority-only, one-time protocol initialization;
- creation of the deployer's config and active admin record;
- super-admin-only add, disable, enable, and remove operations;
- protection against disabling the super-admin or removing an active admin;
- removal and re-creation of a disabled admin record;
- rejection of market creation by non-admin and disabled-admin wallets;
- market initialization followed by order placement;
- market pause and unpause;
- rejection of order placement while paused;
- acceptance of two valid mint accounts;
- rejection of a non-mint account;
- rejection of identical base and quote mints;
- persistence of the selected mint addresses;
- owner-only cancellation of open orders;
- rejection of repeated cancellation and mismatched markets;
- cancellation while the market is paused;
- persistence of mint decimals, tick size, and lot size;
- rejection of zero tick and lot sizes;
- rejection of off-tick prices and off-lot quantities;
- rejection of orders whose notional rounds below one quote atom;
- canonical base/quote vault mints, shared authority, and zero balances;
- atomic rollback when market initialization fails;
- rejection of noncanonical vault accounts and duplicate market initialization;
- base collateral deposits for asks and quote collateral deposits for bids;
- persistence of the exact locked collateral amount;
- rejection of the wrong collateral mint or token-account owner;
- rejection of insufficient balances and noncanonical order vaults;
- rollback of order state, counters, and token balances on failed placement;
- exact base and quote collateral refunds for ask and bid cancellation;
- refunds while a market is paused;
- rejection of wrong refund mints, owners, markets, and vaults;
- atomic preservation of locked collateral after failed cancellation;
- prevention of repeated cancellation and duplicate refunds;
- head, middle, tail, and only-order FIFO removal with reciprocal-link repair;
- checked level count and aggregate quantity decrements;
- best, middle, and worst price-level unlinking with best-pointer advancement;
- empty-level closure and rent return to the recorded payer;
- atomic rejection when a required FIFO or price-level neighbor is omitted;
- rejection of market closure while active or controlled by another signer;
- rejection of shutdown with open bids, open asks, or residual vault balances;
- rejection of noncanonical base and quote vaults during shutdown;
- open-order counter transitions across placement and cancellation;
- atomic closure of an empty paused market and both token vaults;
- deterministic price-level derivation, side separation, and account sizing;
- first bid and first ask price-level creation and best-pointer initialization;
- empty-side bid and ask insertion with no neighbors, plus atomic rejection of an unexpected neighbor;
- new-best, middle, and new-worst insertion for bids and asks;
- atomic rejection of nonadjacent, nonterminal, wrongly ordered, wrong-side,
  wrong-market, stale-best, and noncanonical neighbor hints;
- rejection of a missing current-best neighbor on a non-empty side;
- same-price bid and ask FIFO append, reciprocal links, aggregates, and market
  counters;
- three-order FIFO chaining;
- atomic rejection of stale tails, opposite-side levels, paused-market append,
  and insufficient append collateral.

## 8. Dependency boundary

The program uses Anchor `1.2.0`. Tests use LiteSVM `0.16.0` and its compatible
Solana SDK type family. The direct SDK dependencies are pinned because LiteSVM's
public APIs exchange concrete `Address`, `Message`, `Transaction`, `Signer`, and
`Keypair` types with the test code.

## 9. Planned evolution

The proposed account model and its alternatives are documented in
[`price-level-fifo-design.md`](price-level-fifo-design.md).
The matching and balance model is documented separately in
[`crankless-settlement-design.md`](crankless-settlement-design.md).

The selected direction is bounded crankless PDA matching. The client will
supply a limited sequence of program-maintained price levels, FIFO orders, and
canonical maker balance PDAs. The program will validate the complete path and
atomically update orders plus free/locked trader balances. Informational events
will not represent deferred settlement work.

The recommended implementation order is:

1. ~~Add sorted multi-price insertion.~~
2. <del>Add indexed cancellation/removal.</del>
3. Add canonical per-market trader balances and atomic deposit/withdrawal.
4. Route order collateral through free and locked balance accounting.
5. Implement bounded deterministic matching and atomic ledger settlement.
6. Add partial fills, remainder policy, fees, and conservation tests.
7. Add order cleanup and rent-reclamation rules.

Each phase should add its invariants and failure-path tests before the next
state transition is introduced.
