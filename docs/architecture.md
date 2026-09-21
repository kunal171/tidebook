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
- validate behavior with in-process LiteSVM integration tests.

It does **not** yet refund canceled orders, maintain sorted price levels, match
opposing orders, or settle trades. Orders now hold collateral in market vaults,
but the custody lifecycle is not complete until cancellation can refund it.

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
| `/markets/[address]` | Public viewing; connected wallet for trading | Inspect one market and place bid or ask limit orders while it is active |
| `/admin` | Super-admin | Initialize governance and add, disable, enable, or remove admins |
| `/markets/new` | Active admin or super-admin | Create a market for two SPL mints |
| `/orders` | Any connected wallet | List wallet-owned orders and cancel orders whose status is `Open` |

The orders page filters program accounts by the owner field, displays their
market and order state, and refreshes the list after a confirmed cancellation.

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
| `base_decimals` | Decimal precision read from the base SPL mint during initialization |
| `quote_decimals` | Decimal precision read from the quote SPL mint during initialization |
| `price_tick_size` | Smallest permitted price increment, expressed in raw price units |
| `quantity_lot_size` | Smallest permitted quantity increment, expressed in base-mint atoms |
| `best_bid` | Reserved summary field; currently initialized to `None` and not maintained |
| `best_ask` | Reserved summary field; currently initialized to `None` and not maintained |
| `bump` | Canonical market PDA bump |

### Market vault topology

Each market is created with one PDA authority and two canonical SPL Token
accounts. The authority PDA stores no account data; the program signs for it
with its seeds when token transfers are added in a later milestone.

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
| `quantity` | Base-mint atoms; must be nonzero and aligned to the market lot size |
| `remaining_quantity` | Unfilled quantity; initially equals `quantity` |
| `locked_collateral` | Base atoms for asks or quote atoms for bids currently held in the market vault |
| `status` | Initially `Open`; the owner can transition it to `Canceled`; `Filled` is modeled but not transitioned yet |
| `bump` | Canonical order PDA bump |

Each successful order placement increments `Market.next_order_id`. The order PDA
therefore also provides insertion order that can later support FIFO priority.

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
| `close_market` | Market authority | `has_one = authority`; market is paused | Closes the market account and returns rent to the authority |
| `place_limit_order` | Trader | Market is active; price and quantity are aligned; notional is valid; trader owns the correctly minted token account; collateral vault is canonical; balance is sufficient | Transfers collateral, creates an open order, records the locked amount, and increments `next_order_id` atomically |
| `cancel_limit_order` | Order owner | Order PDA belongs to the supplied market and signer; order status is `Open` | `Open -> Canceled` while preserving `remaining_quantity` |

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

## 6. Known architectural gaps

These are planned features, not defects in the current research milestone:

- Cancellation changes order status but does not yet refund locked collateral.
- No price-level accounts or FIFO order queues.
- No matching engine or partial-fill transitions.
- No settlement or fee accounting.
- `best_bid` and `best_ask` are not updated.
- A paused market can be closed without checking for live order accounts.
- Order accounts are not currently closed or reclaimed.

The close-market rule must be strengthened before this milestone is deployed. A
market with locked funds or live orders must not be closable without a defined
shutdown and withdrawal process, otherwise vault funds can become stranded.

## 7. Test architecture

Integration tests run against LiteSVM in
`programs/tidebook/tests/admin_flow.rs`,
`programs/tidebook/tests/cancel_order.rs`, and
`programs/tidebook/tests/order_flow.rs`.

The test harness:

1. loads the compiled SBF program into an in-process VM;
2. funds a generated payer;
3. inserts rent-exempt SPL Mint fixtures with valid packed mint state;
4. constructs Anchor instructions and versioned transactions;
5. sends transactions through LiteSVM;
6. deserializes resulting Anchor accounts and checks state.

The 40-test suite currently covers:

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
- rollback of order state, counters, and token balances on failed placement.

## 8. Dependency boundary

The program uses Anchor `1.2.0`. Tests use LiteSVM `0.16.0` and its compatible
Solana SDK type family. The direct SDK dependencies are pinned because LiteSVM's
public APIs exchange concrete `Address`, `Message`, `Transaction`, `Signer`, and
`Keypair` types with the test code.

## 9. Planned evolution

The recommended implementation order is:

1. Refund locked collateral when an open order is canceled.
2. Prevent market closure while live orders or vault balances remain.
3. Add price-level accounts and FIFO queues.
4. Implement deterministic matching and partial fills.
5. Settle base/quote transfers and fees.
6. Add safe market shutdown, vault closure, order cleanup, and withdrawal rules.

Each phase should add its invariants and failure-path tests before the next
state transition is introduced.
