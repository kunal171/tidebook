//! Governance initialization and permissionless claims for valueless test assets.
//!
//! Solana programs cannot detect whether they execute on devnet or mainnet.
//! Consequently, this faucet is opt-in: the super-admin must explicitly select
//! a zero-supply market pair and transfer both mint authorities to a program
//! PDA. Never initialize this account for assets carrying real economic value.

use anchor_lang::{prelude::*, solana_program::program_option::COption};
use anchor_spl::token::{
    self, spl_token::instruction::AuthorityType, Mint, MintTo, SetAuthority, Token, TokenAccount,
};

use crate::{
    constants::{PROTOCOL_CONFIG_SEED, TEST_FAUCET_AUTHORITY_SEED, TEST_FAUCET_SEED},
    errors::faucet,
    events::{TestFaucetClaimedEvent, TestFaucetInitializedEvent},
    state::{Market, ProtocolConfig, TestFaucet},
};

#[derive(Accounts)]
pub struct InitializeTestFaucet<'info> {
    #[account(mut)]
    pub super_admin: Signer<'info>,

    #[account(
        seeds = [PROTOCOL_CONFIG_SEED],
        bump = protocol_config.bump,
        constraint = protocol_config.super_admin == super_admin.key()
            @ faucet::UnauthorizedSuperAdmin
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,

    pub market: Account<'info, Market>,

    #[account(
        init,
        payer = super_admin,
        space = 8 + TestFaucet::INIT_SPACE,
        seeds = [TEST_FAUCET_SEED, market.key().as_ref()],
        bump
    )]
    pub test_faucet: Account<'info, TestFaucet>,

    /// CHECK: Canonical stateless PDA that becomes both mint authorities.
    #[account(
        seeds = [TEST_FAUCET_AUTHORITY_SEED, test_faucet.key().as_ref()],
        bump
    )]
    pub faucet_authority: UncheckedAccount<'info>,

    #[account(
        mut,
        constraint = base_mint.key() == market.base_mint @ faucet::FaucetMintMismatch
    )]
    pub base_mint: Account<'info, Mint>,

    #[account(
        mut,
        constraint = quote_mint.key() == market.quote_mint @ faucet::FaucetMintMismatch
    )]
    pub quote_mint: Account<'info, Mint>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

pub fn handle_initialize_test_faucet(
    ctx: Context<InitializeTestFaucet>,
    base_claim_amount: u64,
    quote_claim_amount: u64,
) -> Result<()> {
    require!(
        base_claim_amount > 0 && quote_claim_amount > 0,
        faucet::InvalidFaucetClaimAmount
    );
    require!(
        ctx.accounts.base_mint.supply == 0 && ctx.accounts.quote_mint.supply == 0,
        faucet::FaucetMintSupplyNotZero
    );

    let super_admin = ctx.accounts.super_admin.key();
    require!(
        ctx.accounts.base_mint.mint_authority == COption::Some(super_admin)
            && ctx.accounts.quote_mint.mint_authority == COption::Some(super_admin),
        faucet::InvalidFaucetMintAuthority
    );

    let faucet_authority = ctx.accounts.faucet_authority.key();
    for mint in [
        ctx.accounts.base_mint.to_account_info(),
        ctx.accounts.quote_mint.to_account_info(),
    ] {
        token::set_authority(
            CpiContext::new(
                ctx.accounts.token_program.key(),
                SetAuthority {
                    current_authority: ctx.accounts.super_admin.to_account_info(),
                    account_or_mint: mint,
                },
            ),
            AuthorityType::MintTokens,
            Some(faucet_authority),
        )?;
    }

    let test_faucet = &mut ctx.accounts.test_faucet;
    test_faucet.market = ctx.accounts.market.key();
    test_faucet.base_mint = ctx.accounts.base_mint.key();
    test_faucet.quote_mint = ctx.accounts.quote_mint.key();
    test_faucet.base_claim_amount = base_claim_amount;
    test_faucet.quote_claim_amount = quote_claim_amount;
    test_faucet.authority_bump = ctx.bumps.faucet_authority;
    test_faucet.bump = ctx.bumps.test_faucet;

    emit!(TestFaucetInitializedEvent {
        market: test_faucet.market,
        test_faucet: test_faucet.key(),
        faucet_authority,
        base_mint: test_faucet.base_mint,
        quote_mint: test_faucet.quote_mint,
        base_claim_amount,
        quote_claim_amount,
    });

    Ok(())
}

#[derive(Accounts)]
pub struct CloseTestFaucet<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(has_one = authority)]
    pub market: Account<'info, Market>,

    #[account(
        mut,
        close = authority,
        seeds = [TEST_FAUCET_SEED, market.key().as_ref()],
        bump = test_faucet.bump,
        constraint = test_faucet.market == market.key() @ faucet::FaucetMarketMismatch,
        constraint = test_faucet.base_mint == market.base_mint
            @ faucet::FaucetMintMismatch,
        constraint = test_faucet.quote_mint == market.quote_mint
            @ faucet::FaucetMintMismatch
    )]
    pub test_faucet: Account<'info, TestFaucet>,
}

/// Permanently disables public minting for this research market.
///
/// The stateless authority PDA cannot sign outside Tidebook, and every claim
/// requires the configuration account. Closing the config therefore freezes
/// both test-mint supplies without needing to transfer authority elsewhere.
pub fn handle_close_test_faucet(ctx: Context<CloseTestFaucet>) -> Result<()> {
    emit!(crate::events::TestFaucetClosedEvent {
        market: ctx.accounts.market.key(),
        test_faucet: ctx.accounts.test_faucet.key(),
        authority: ctx.accounts.authority.key(),
        base_mint: ctx.accounts.test_faucet.base_mint,
        quote_mint: ctx.accounts.test_faucet.quote_mint,
    });

    Ok(())
}

#[derive(Accounts)]
pub struct ClaimTestTokens<'info> {
    pub claimant: Signer<'info>,

    pub market: Account<'info, Market>,

    #[account(
        seeds = [TEST_FAUCET_SEED, market.key().as_ref()],
        bump = test_faucet.bump,
        constraint = test_faucet.market == market.key() @ faucet::FaucetMarketMismatch
    )]
    pub test_faucet: Account<'info, TestFaucet>,

    /// CHECK: Canonical PDA that signs both mint-to CPIs.
    #[account(
        seeds = [TEST_FAUCET_AUTHORITY_SEED, test_faucet.key().as_ref()],
        bump = test_faucet.authority_bump
    )]
    pub faucet_authority: UncheckedAccount<'info>,

    #[account(
        mut,
        constraint = base_mint.key() == market.base_mint @ faucet::FaucetMintMismatch,
        constraint = base_mint.key() == test_faucet.base_mint @ faucet::FaucetMintMismatch
    )]
    pub base_mint: Account<'info, Mint>,

    #[account(
        mut,
        constraint = quote_mint.key() == market.quote_mint @ faucet::FaucetMintMismatch,
        constraint = quote_mint.key() == test_faucet.quote_mint @ faucet::FaucetMintMismatch
    )]
    pub quote_mint: Account<'info, Mint>,

    #[account(
        mut,
        constraint = claimant_base_account.owner == claimant.key()
            @ faucet::InvalidFaucetDestinationOwner,
        constraint = claimant_base_account.mint == base_mint.key()
            @ faucet::FaucetMintMismatch
    )]
    pub claimant_base_account: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = claimant_quote_account.owner == claimant.key()
            @ faucet::InvalidFaucetDestinationOwner,
        constraint = claimant_quote_account.mint == quote_mint.key()
            @ faucet::FaucetMintMismatch
    )]
    pub claimant_quote_account: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
}

pub fn handle_claim_test_tokens(ctx: Context<ClaimTestTokens>) -> Result<()> {
    let faucet_key = ctx.accounts.test_faucet.key();
    let authority_bump = [ctx.accounts.test_faucet.authority_bump];
    let authority_seeds: &[&[u8]] = &[
        TEST_FAUCET_AUTHORITY_SEED,
        faucet_key.as_ref(),
        &authority_bump,
    ];
    let signer_seeds = &[authority_seeds];

    for (mint, destination, amount) in [
        (
            ctx.accounts.base_mint.to_account_info(),
            ctx.accounts.claimant_base_account.to_account_info(),
            ctx.accounts.test_faucet.base_claim_amount,
        ),
        (
            ctx.accounts.quote_mint.to_account_info(),
            ctx.accounts.claimant_quote_account.to_account_info(),
            ctx.accounts.test_faucet.quote_claim_amount,
        ),
    ] {
        token::mint_to(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.key(),
                MintTo {
                    mint,
                    to: destination,
                    authority: ctx.accounts.faucet_authority.to_account_info(),
                },
                signer_seeds,
            ),
            amount,
        )?;
    }

    emit!(TestFaucetClaimedEvent {
        market: ctx.accounts.market.key(),
        test_faucet: faucet_key,
        claimant: ctx.accounts.claimant.key(),
        base_amount: ctx.accounts.test_faucet.base_claim_amount,
        quote_amount: ctx.accounts.test_faucet.quote_claim_amount,
    });

    Ok(())
}
