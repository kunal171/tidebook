# Devnet Address Registry

This file records the shared Solana devnet addresses used while developing and
testing Tidebook. These accounts have no mainnet value and must not be reused as
production configuration.

## Program and governance

| Account | Address | Notes |
| --- | --- | --- |
| Tidebook program | `Honq7kkNfptR6XF5H4zn2jWqSmNRsteCpGwB8iG393cR` | Upgradeable devnet program |
| ProgramData | `4419TJGH2EMxFhpfpz1CANZrvyxirCjRbjMEMJeAhzPt` | Upgradeable loader metadata and program data |
| Upgrade authority | `BX6RJHGbi7msj7t1ECCX6T1ZvvetHDK6UkjzAhPfWngq` | Protocol deployer and super-admin |
| Protocol config PDA | `Gymt28bxrkXFxXnNhnbFgW34oP91u6r7iRgKMKTTF7XH` | Seeds: `["protocol_config"]` |
| Super-admin record PDA | `DXLqsoioXX157XFgSZLv5gNR5bvbKkfNtbdSf6Fa7QE6` | Seeds: `["admin", upgrade_authority]` |

## Test token mints

Both mints use the legacy SPL Token Program
`TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA`, which is the mint owner expected
by the current Tidebook program.

| Test asset | Role | Mint address | Decimals | Current supply |
| --- | --- | --- | --- | --- |
| Mock BTC (`tBTC`) | Base mint | `FggaZG7eJ5tpr2vmpNMbMCHkWkcw9n2Nhmf54g3wQwGk` | 8 | 0 |
| Mock USDT (`tUSDT`) | Quote mint | `8KRa3QwidV2yZspR3wr4p3eNzq2KtejMbU1tZUgNLdtt` | 6 | 0 |

Mint authority for both test assets:
`BX6RJHGbi7msj7t1ECCX6T1ZvvetHDK6UkjzAhPfWngq`.

### Creation transactions

| Test asset | Devnet transaction |
| --- | --- |
| Mock BTC | `4qsjj1vteYmWyQeMb6MPBfU2D4y2nwroTzVFvaTwC1PtobKML6X5GB13EU1HQ7j2LUn6jo3ey3CCeyfDj9ZakfuT` |
| Mock USDT | `XrdHiMNWFa2PcJCAUcwwRJnRKxszkKXh8V4ZdxA4hCZNdcHFQcL29R6cBFh2xJmCpiJkuX9rQKGPMUpVDA2i5Qr` |

## Markets

| Pair | Market PDA | Status | Creation transaction |
| --- | --- | --- | --- |
| `tBTC/tUSDT` | `DMkKmZ44C8rvA4z2PeGDYmj73HHeCfXbBCVDPqCA9ecb` | Active | `24T3QZuGX6qvZPbLYQgHgvinYvxiR8qpQNq7yZ2fmeaiKsNXffGpoujYC6KRab5tj82kydmjTYN5wTbDCMZthWuw` |

The PDA uses the ordered seeds `["market", tBTC_mint, tUSDT_mint]`. Reversing
the mint order produces a different market address.

## Maintenance rules

- Record only public addresses and transaction signatures.
- Never add seed phrases, private keys, or keypair file contents.
- State the network explicitly when adding an account.
- Record token program, decimals, and intended role for every new test mint.
- Update this file whenever shared devnet accounts are created or replaced.
