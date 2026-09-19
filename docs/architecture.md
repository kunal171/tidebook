# Tidebook Architecture

## 1. Purpose and current scope

`tidebook` is an Anchor program being developed as a research study of an
on-chain central limit order book.

The current implementation establishes the account and authorization foundation:

- create one deterministic market for a distinct SPL base/quote mint pair;
- pause, unpause, and close a market under authority control;
- create deterministic limit-order accounts while a market is active;
- validate behavior with in-process LiteSVM integration tests.

It does **not** yet lock tokens, maintain sorted price levels, match opposing
orders, cancel orders, or settle trades. An `Order` currently records intent; it
is not a collateralized order.

## 2. System context

The program has two external roles:

| Role | Current capabilities |
| --- | --- |
| Market authority | Initialize a market, pause it, unpause it, and close it while paused |
| Trader | Place a bid or ask limit order while the market is active |

The program reads SPL Token mint accounts during market initialization. It does
not currently invoke the Token Program or transfer any tokens.

### Companion web application

The `app/` workspace is a Vite, React, and TypeScript client. Its first
milestone connects browser wallets, targets devnet, and checks the configured
program account through Solana RPC. The program is live on devnet. Transaction
controls remain disabled until the generated Anchor IDL is connected to the app.

The client keeps network and program identity in `app/src/config/solana.ts`.
The program address must remain synchronized with `declare_id!`, `Anchor.toml`,
and the deployment keypair.

The editable system diagram is available at
[`diagrams/tidebook-architecture.drawio`](diagrams/tidebook-architecture.drawio).

## 3. On-chain account model

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
| `status` | Initially `Open`; `Filled` and `Canceled` are modeled but not transitioned yet |
| `bump` | Canonical order PDA bump |

Each successful order placement increments `Market.next_order_id`. The order PDA
therefore also provides insertion order that can later support FIFO priority.

## 4. Instruction architecture

| Instruction | Required signer | Important checks | State transition |
| --- | --- | --- | --- |
| `initialize_market` | Authority | Both accounts deserialize as SPL mints; base and quote differ; market PDA is canonical | Creates an active market with `next_order_id = 1` |
| `pause_market` | Market authority | `has_one = authority`; market is active | `Active -> Paused` |
| `unpause_market` | Market authority | `has_one = authority`; market is paused | `Paused -> Active` |
| `close_market` | Market authority | `has_one = authority`; market is paused | Closes the market account and returns rent to the authority |
| `place_limit_order` | Trader | Market is active; price and quantity are nonzero; order PDA is canonical | Creates an open order and increments `next_order_id` |

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

## 6. Known architectural gaps

These are planned features, not defects in the current research milestone:

- No trader token-account validation or asset custody.
- No base or quote vault PDAs.
- No tick-size, lot-size, overflow, or notional-value rules.
- No price-level accounts or FIFO order queues.
- No matching engine or partial-fill transitions.
- No cancel-order instruction.
- No settlement or fee accounting.
- `best_bid` and `best_ask` are not updated.
- A paused market can be closed without checking for live order accounts.
- Order accounts are not currently closed or reclaimed.

The close-market rule must be strengthened before custody is introduced. A
market with locked funds or live orders must not be closable without a defined
shutdown and withdrawal process.

## 7. Test architecture

Integration tests run against LiteSVM in
`programs/tidebook/tests/order_flow.rs`.

The test harness:

1. loads the compiled SBF program into an in-process VM;
2. funds a generated payer;
3. inserts rent-exempt SPL Mint fixtures with valid packed mint state;
4. constructs Anchor instructions and versioned transactions;
5. sends transactions through LiteSVM;
6. deserializes resulting Anchor accounts and checks state.

The suite currently covers:

- market initialization followed by order placement;
- market pause and unpause;
- rejection of order placement while paused;
- acceptance of two valid mint accounts;
- rejection of a non-mint account;
- rejection of identical base and quote mints;
- persistence of the selected mint addresses.

## 8. Dependency boundary

The program uses Anchor `1.2.0`. Tests use LiteSVM `0.16.0` and its compatible
Solana SDK type family. The direct SDK dependencies are pinned because LiteSVM's
public APIs exchange concrete `Address`, `Message`, `Transaction`, `Signer`, and
`Keypair` types with the test code.

## 9. Planned evolution

The recommended implementation order is:

1. Wire the generated Anchor IDL into the app and enable market creation.
2. Add `cancel_limit_order` and complete the basic order lifecycle in program,
   tests, and UI.
3. Define price ticks, quantity lots, and checked arithmetic rules.
4. Add market vault authorities and base/quote token vaults.
5. Lock the correct asset when a bid or ask is placed.
6. Add price-level accounts and FIFO queues.
7. Implement deterministic matching and partial fills.
8. Settle base/quote transfers and fees.
9. Add safe market shutdown, order cleanup, and withdrawal rules.

Each phase should add its invariants and failure-path tests before the next
state transition is introduced.
