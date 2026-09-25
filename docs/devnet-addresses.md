# Devnet Address Registry

This file records the shared Solana devnet addresses used while developing and
testing Tidebook. These accounts have no mainnet value and must not be reused as
production configuration.

## Program and governance

| Account | Address | Notes |
| --- | --- | --- |
| Tidebook program | `BPdNF5CnV8z1EkHo7tcueR6wXmzZV2j6j4wsUTirPgWL` | Current upgradeable devnet program; multi-maker build verified at slot `503898678` |
| ProgramData | `HvXrEzK1NeTQg3PhRsVFXPyshAe6mj4L5PwK132xn26g` | Upgradeable loader metadata and program data |
| Upgrade authority | `BX6RJHGbi7msj7t1ECCX6T1ZvvetHDK6UkjzAhPfWngq` | Protocol deployer and super-admin |
| Protocol config PDA | `4zjC2AkEeCqWAc2Hbytt9envB4VP8EYtz9vGk9MFvUsP` | Seeds: `["protocol_config"]` |
| Super-admin record PDA | `6hMnArFtd6UigUTtBW5283v1PEJps2ofUfJRozuKNtcJ` | Seeds: `["admin", upgrade_authority]` |

Governance initialization transaction:
`55rkrHeyYcVksfVxmpChDbSEfuuDAsSyFkWPd7StyKUQQ1VWmmFuYuTHBZVgTXsSm9xsTag8RDtKsdpQ32renNNM`.

## Test token mints

Both mints use the legacy SPL Token Program
`TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA`, which is the mint owner expected
by the current Tidebook program.

| Test asset | Role | Mint address | Decimals | Current supply |
| --- | --- | --- | --- | --- |
| Mock BTC (`tBTC`) | Base mint | `FggaZG7eJ5tpr2vmpNMbMCHkWkcw9n2Nhmf54g3wQwGk` | 8 | 0 |
| Mock USDT (`tUSDT`) | Quote mint | `8KRa3QwidV2yZspR3wr4p3eNzq2KtejMbU1tZUgNLdtt` | 6 | 0 |
| Multi-maker base | Smoke-test base | `DS4R9TLJmdnQXS1VqWbzXBE88LT4QzB52CjDCXKW5hr7` | 8 | 2 |
| Multi-maker quote | Smoke-test quote | `6XGySXZ2Soe8DyNJ6Zs3efAKpigrz1RJWWYLkq1eMvuJ` | 6 | 1,000 |

Mint authority for both test assets:
`BX6RJHGbi7msj7t1ECCX6T1ZvvetHDK6UkjzAhPfWngq`.

### Creation transactions

| Test asset | Devnet transaction |
| --- | --- |
| Mock BTC | `4qsjj1vteYmWyQeMb6MPBfU2D4y2nwroTzVFvaTwC1PtobKML6X5GB13EU1HQ7j2LUn6jo3ey3CCeyfDj9ZakfuT` |
| Mock USDT | `XrdHiMNWFa2PcJCAUcwwRJnRKxszkKXh8V4ZdxA4hCZNdcHFQcL29R6cBFh2xJmCpiJkuX9rQKGPMUpVDA2i5Qr` |
| Multi-maker base | `mpTMW6r5ATui2pA6j4vgcBSh9UBnVr3v4NMmhpW4NjT3h7TMYY56LLXcZqfPbaapn9Ynheh5Keh9QdjM6RgT3zL` |
| Multi-maker quote | `akBppZeJMg7FMSG3FK4WFLwNiPMhUKxBmNnFLWv4mQrqCvjaD4jEy6P56i1r6u7DZ6L5iDEkWjptLUW6r6SUUt8` |

## Markets

| Pair | Market PDA | Status | Creation transaction |
| --- | --- | --- | --- |
| `tBTC/tUSDT` | `9t1vydxLFBzUFrh5v7CE4nPgu46v4Nk7uPFcPmk69kqa` | Pre-`open_order_count` layout; unsupported historical account | `4jNDzMMnHYx8v6C5yWT5KzCKPQu43kWZ2Xmt75YiFTGsc5XYj2BcmQLipM8X5SPBmtSDHVm6G9gjK5AVm9enqE4M` |
| Multi-maker smoke | `EXFUazTspBvNDm4D2iGwpYVuT7R9BGLs16koKLhz6Viz` | Current layout; active and empty after smoke run; tick/lot `10000`/`100000` | `qvhtFrJ3UokdMtA9byKsAbgfyxVA3dStLuNRVCJtg7FSUNJh8DwrDpvrbZYjoraFfXnnBaKE5cXXsPdQNZ5i9RD` |

The PDA uses the ordered seeds `["market", tBTC_mint, tUSDT_mint]`. Reversing
the mint order produces a different market address.

### Price-model smoke order

| Account/action | Address or transaction |
| --- | --- |
| Order PDA (`#1`) | `ALqkALSYnBxvNSYGAMwQqocB3EH2hEFCeWzP44zvGg8k` |
| Place order | `2cXeTxAuc4VP2vwzpnXHzKhKbNuxvoUQAhDEYpdLRk85EwP7dPwy4wS5bBPrPYLz6g8aJUofYX5Ed9XUkBmPvygo` |
| Cancel order | `237sGm5DnC9zs3w4RFxyujKNEZMCrNvf97FvcuJwgJsh6TLEYQGUhA4ZK85VmfbZjVBevCz4dacXPt6aCcGGJave` |

The smoke order used price `67250120000` and quantity `100000`. Its final
on-chain status is `Canceled`. Separate simulations confirmed that off-tick and
off-lot values fail with `PriceNotOnTick` and `QuantityNotOnLot`.

### Atomic multi-maker smoke run

The current-layout market was tested with two maker wallets and one taker:

| Item | Address |
| --- | --- |
| First maker | `BX6RJHGbi7msj7t1ECCX6T1ZvvetHDK6UkjzAhPfWngq` |
| Second maker | `63zrsd7vEEdh8UJgZSHDKtRQEZ3NCV6xxFME1xzdUJhK` |
| Taker | `HJxRvXQGgKBV7VuMLp8Hkxz1wz4hERYEMBdVfhZEn1xW` |
| First order | `86ATXUVWnBAMNgQr1RdT7PkWXNHDbrwwX1iDMKJNJcRq` |
| Second order | `5YhA5bQgDfphQpuyv8x4jg1B5uEWcGGCfLsAPZ66S3bg` |
| Closed price level | `7CDGn2rnBejw6UtFAQPDz86BxcjmDiA3hand6vKB4yGq` |

| Action | Devnet transaction |
| --- | --- |
| First maker placement | `4oRTrYdfCvgFLLPEfKpJRuLZBoKsJZtwLVUvTiQGEHd2FvmMEK1jPtntDazPriTLRwZQ4zsbAkiHi493PLbEDQmb` |
| Second FIFO append | `Ux93N5p9EbkAJFh6Yjkd5AacMkAWQgPXsi1EAHevJ3riUBxPUKYieM5uaFsxg9wwVKsvUWRF2xrmPA6zFCbuduG` |
| Two-instruction atomic match | `5W63pkUniTki57fXu4JomMbEZT1WS2eSGrmU7SSTW8xnW2rdCyHYqR91a4N3vmS2gyNidBfgEpGGgxbbD4L7m5CP` |

The match transaction contains two consecutive `MatchLimitOrder` instructions.
Both `100000`-unit asks were filled at maker price `100000000`. The final
market has no best ask and zero open orders, the empty level is closed, both
makers received `100000` quote atoms, and the taker ended with `200000` free
base atoms and `800000` free quote atoms. The reusable runner is
`app/scripts/devnet-multi-maker-smoke.mjs`; it requires fresh isolated mints
and funded token accounts supplied through environment variables.

## Legacy deployment

Program `Honq7kkNfptR6XF5H4zn2jWqSmNRsteCpGwB8iG393cR` and market
`DMkKmZ44C8rvA4z2PeGDYmj73HHeCfXbBCVDPqCA9ecb` use the earlier market account
layout. They are retained only as devnet research history and are unsupported by
the current application IDL.

## Maintenance rules

- Record only public addresses and transaction signatures.
- Never add seed phrases, private keys, or keypair file contents.
- State the network explicitly when adding an account.
- Record token program, decimals, and intended role for every new test mint.
- Update this file whenever shared devnet accounts are created or replaced.
