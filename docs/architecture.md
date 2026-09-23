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
- create the first canonical price level on each side and append a same-price
  order behind a validated FIFO tail;
- validate behavior with in-process LiteSVM integration tests.

The program does **not** yet insert multiple sorted prices, unlink indexed
orders during cancellation, match opposing orders, or settle trades. Orders
hold collateral in market vaults, and canceling an open order refunds its entire
locked amount to an owner-controlled token account. Price-level data must be
treated as research-stage until cancellation maintains the same index.

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
passes the canonical vault accounts to the program. The orders page filters
program accounts by owner, displays locked collateral, returns it to an owned
token account during cancellation, and refreshes after confirmation.

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
payer, and canonical bump. The first-level creation path initializes the queue
with one order. `append_limit_order` validates the current tail and atomically
links a second same-price order behind it.

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
| `place_limit_order` | Trader | Market is active and the selected side has no level; price and quantity are aligned; collateral accounts are canonical and funded | Atomically transfers collateral, creates the first level and order, sets the side's best-price pointer, and increments counters |
| `append_limit_order` | Trader | Existing level is canonical for market/side/price; supplied previous order is its open tail with no successor; collateral validation matches placement | Atomically transfers collateral, creates an order, links it behind the tail, and updates level aggregates and market counters |
| `cancel_limit_order` | Order owner | Order belongs to the supplied market and signer; status is `Open`; refund account belongs to the owner and uses the side's collateral mint; vault is canonical | Refunds `locked_collateral`, sets it to zero, and transitions `Open -> Canceled` even while paused |

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

### Limit-order placement flow

```text
Trader
   |
   | place_limit_order(side, price, quantity)
   v
Validate active Market + nonzero values
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
Validate ownership + Open status
   |-- bid selects quote mint and quote vault
   |-- ask selects base mint and base vault
   |-- destination belongs to the order owner
   |-- vault matches ["vault", market, collateral mint]
   v
Vault-authority PDA signs the collateral refund
   |
   v
Set locked_collateral = 0 and status = Canceled
```

Cancellation deliberately has no active-market requirement, preserving the
owner's exit path while a market is paused. Transfer and state changes are one
atomic transaction, so a failed refund leaves the order open and funded.

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
33. A second distinct price is rejected until sorted insertion is implemented,
    preventing accidental replacement of the current best pointer.
34. An existing-level append accepts only the level's open tail with no newer
    successor.
35. FIFO tail linking, level aggregates, collateral transfer, and market
    counters are updated atomically.

## 6. Known architectural gaps

These are planned features, not defects in the current research milestone:

- No sorted insertion of a second distinct price on either side.
- Cancellation does not yet unlink orders, update aggregates, or close an empty
  level; price-level state can therefore be stale after cancellation.
- No matching engine or partial-fill transitions.
- No settlement or fee accounting.
- Best-price pointers do not yet advance between sorted levels.
- Order accounts are not currently closed or reclaimed.

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

The 70-test suite currently covers:

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
- rejection of market closure while active or controlled by another signer;
- rejection of shutdown with open bids, open asks, or residual vault balances;
- rejection of noncanonical base and quote vaults during shutdown;
- open-order counter transitions across placement and cancellation;
- atomic closure of an empty paused market and both token vaults;
- deterministic price-level derivation, side separation, and account sizing;
- first bid and first ask price-level creation and best-pointer initialization;
- safe rejection of a second distinct bid price before sorted insertion exists;
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

1. Add sorted multi-price insertion and indexed cancellation/removal.
2. Add canonical per-market trader balances and atomic deposit/withdrawal.
3. Route order collateral through free and locked balance accounting.
4. Implement bounded deterministic matching and atomic ledger settlement.
5. Add partial fills, remainder policy, fees, and conservation tests.
6. Add order cleanup and rent-reclamation rules.

Each phase should add its invariants and failure-path tests before the next
state transition is introduced.
