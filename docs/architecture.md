# Tidebook Architecture

## 1. Purpose and current scope

`tidebook` is an Anchor program being developed as a research study of an
on-chain central limit order book.

The current implementation establishes the account and authorization foundation:

- initialize protocol governance from the program upgrade authority;
- manage deterministic, independently removable administrator records;
- create one deterministic market for a distinct SPL base/quote mint pair;
- pause, unpause, and close a market under authority control;
- create deterministic limit-order accounts while a market is active;
- validate behavior with in-process LiteSVM integration tests.

It does **not** yet lock tokens, maintain sorted price levels, match opposing
orders, or settle trades. An `Order` currently records intent; it is not a
collateralized order.

## 2. System context

The program has three external roles:

| Role | Current capabilities |
| --- | --- |
| Super-admin | Add, disable, enable, and remove administrator records |
| Market authority | Initialize a market, pause it, unpause it, and close it while paused |
| Trader | Place a bid or ask limit order while the market is active |

The program reads SPL Token mint accounts during market initialization. It does
not currently invoke the Token Program or transfer any tokens.

### Companion web application

The `app/` workspace is a Next.js App Router, React, and TypeScript client. It
connects browser wallets to devnet and uses a checked-in copy of the generated
Anchor IDL at `app/idl/tidebook.json`.

The shared role provider reads the protocol config and the connected wallet's
admin-record PDA. The header shows a `Super admin` or `Admin` badge only for an
active on-chain record. Role-aware routes are:

| Route | UI access | Purpose |
| --- | --- | --- |
| `/admin` | Super-admin | Initialize governance and add, disable, enable, or remove admins |
| `/markets/new` | Active admin or super-admin | Create a market for two SPL mints |

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
| `best_bid` | Reserved summary field; currently initialized to `None` and not maintained |
| `best_ask` | Reserved summary field; currently initialized to `None` and not maintained |
| `bump` | Canonical market PDA bump |

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
| `price` | Raw integer limit price; must be greater than zero |
| `quantity` | Original raw integer quantity; must be greater than zero |
| `remaining_quantity` | Unfilled quantity; initially equals `quantity` |
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
| `initialize_market` | Active admin | Admin record belongs to signer, is canonical and active; both accounts deserialize as SPL mints; base and quote differ; market PDA is canonical | Creates an active market with `next_order_id = 1` |
| `pause_market` | Market authority | `has_one = authority`; market is active | `Active -> Paused` |
| `unpause_market` | Market authority | `has_one = authority`; market is paused | `Paused -> Active` |
| `close_market` | Market authority | `has_one = authority`; market is paused | Closes the market account and returns rent to the authority |
| `place_limit_order` | Trader | Market is active; price and quantity are nonzero; order PDA is canonical | Creates an open order and increments `next_order_id` |
| `cancel_limit_order` | Order owner | Order PDA belongs to the supplied market and signer; order status is `Open` | `Open -> Canceled` while preserving `remaining_quantity` |

### Market initialization flow

```text
Authority
   |
   | initialize_market(base mint, quote mint)
   v
Anchor account validation
   |-- base account is an SPL Mint
   |-- quote account is an SPL Mint
   |-- base mint != quote mint
   |-- market PDA matches the ordered pair
   v
Market PDA created as Active
```

### Limit-order placement flow

```text
Trader
   |
   | place_limit_order(side, price, quantity)
   v
Validate active Market + nonzero values
   |
   | derive ["order", market, next_order_id]
   v
Create Order PDA as Open
   |
   v
Increment Market.next_order_id
```

This operation is atomic: if account creation or validation fails, the market
counter and order state are not committed.

## 5. Current invariants

The program currently enforces:

1. A market is identified by its ordered pair of mint addresses.
2. Both market assets are initialized SPL Token mint accounts.
3. Base and quote mint addresses must be different.
4. Only the stored authority can pause, unpause, or close the market.
5. A market must be active to accept a new order.
6. A market must be paused before it can be closed.
7. Price and quantity must both be nonzero.
8. An order ID is allocated only by incrementing its market's counter.
9. Only the configured super-admin can manage administrator records.
10. The super-admin cannot disable its own admin record.
11. An administrator must be disabled before its record can be removed.
12. Only a signer with its canonical active admin record can initialize a market.
13. Only the stored order owner can cancel an order.
14. Only an `Open` order can transition to `Canceled`.

## 6. Known architectural gaps

These are planned features, not defects in the current research milestone:

- No trader token-account validation or asset custody.
- No base or quote vault PDAs.
- No tick-size, lot-size, overflow, or notional-value rules.
- No price-level accounts or FIFO order queues.
- No matching engine or partial-fill transitions.
- No settlement or fee accounting.
- `best_bid` and `best_ask` are not updated.
- A paused market can be closed without checking for live order accounts.
- Order accounts are not currently closed or reclaimed.

The close-market rule must be strengthened before custody is introduced. A
market with locked funds or live orders must not be closable without a defined
shutdown and withdrawal process.

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

The 25-test suite currently covers:

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
- cancellation while the market is paused.

## 8. Dependency boundary

The program uses Anchor `1.2.0`. Tests use LiteSVM `0.16.0` and its compatible
Solana SDK type family. The direct SDK dependencies are pinned because LiteSVM's
public APIs exchange concrete `Address`, `Message`, `Transaction`, `Signer`, and
`Keypair` types with the test code.

## 9. Planned evolution

The recommended implementation order is:

1. Add order discovery and cancellation controls to the UI.
2. Define price ticks, quantity lots, and checked arithmetic rules.
3. Add market vault authorities and base/quote token vaults.
4. Lock the correct asset when a bid or ask is placed.
5. Add price-level accounts and FIFO queues.
6. Implement deterministic matching and partial fills.
7. Settle base/quote transfers and fees.
8. Add safe market shutdown, order cleanup, and withdrawal rules.

Each phase should add its invariants and failure-path tests before the next
state transition is introduced.
