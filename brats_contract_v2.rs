
// SPDX-License-Identifier: MIT
// $BRATS Smart Contract - Solana (Rust & Anchor Framework)

use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount, Mint, Transfer, Burn};

declare_id!("BRATS_PROGRAM_ID_PLACEHOLDER");

// Constants
const TRANSACTION_FEE_PERCENT: u64 = 3;
const BURN_PERCENT: u64 = 1;
const REWARD_POOL_PERCENT: u64 = 10;
const APY: u64 = 43;
const STAKING_DURATION: i64 = 180 * 24 * 3600; // 6 months
const EARLY_UNSTAKE_PERIOD: i64 = 7 * 24 * 3600; // 7-day lock after launch before early unstake is allowed
const LIQUIDITY_LOCK_PERIOD: i64 = 365 * 24 * 3600; // 1 Year
const MULTI_SIG_WALLET: &str = "6oUXG2nTxLXC9UNJuj1Q6pumPSm1oqE9JyJiFQMXNZEQ";

#[account]
pub struct PresaleState {
    pub is_presale_active: bool,
    pub presale_end_time: Option<i64>,
    pub launch_time: Option<i64>,
}

#[program]
pub mod brats_contract {
    use super::*;

    pub fn initialize_token(ctx: Context<InitializeToken>) -> ProgramResult {
        let presale_state = &mut ctx.accounts.presale_state;
        presale_state.is_presale_active = true;
        presale_state.presale_end_time = None;
        presale_state.launch_time = None;
        Ok(())
    }

    pub fn end_presale(ctx: Context<EndPresale>) -> ProgramResult {
        let presale_state = &mut ctx.accounts.presale_state;
        require!(presale_state.is_presale_active, ErrorCode::PresaleAlreadyEnded);
        let clock = Clock::get()?;
        presale_state.is_presale_active = false;
        presale_state.presale_end_time = Some(clock.unix_timestamp);
        presale_state.launch_time = Some(clock.unix_timestamp);
        Ok(())
    }

    pub fn accept_payment(ctx: Context<AcceptPayment>, amount: u64, token_mint: Pubkey) -> ProgramResult {
        if token_mint == Pubkey::default() {
            // Handle SOL payment
            require!(amount > 0, ErrorCode::InvalidAmount);
        } else {
            // Handle USDT/USDC SPL token payments
            require!(ctx.accounts.token_account.amount >= amount, ErrorCode::InsufficientFunds);
            token::transfer(ctx.accounts.transfer_context(), amount)?;
        }
        Ok(())
    }

    
pub fn stake_tokens(ctx: Context<StakeTokens>, amount: u64) -> ProgramResult {
    let presale_state = &ctx.accounts.presale_state;
    require!(presale_state.is_presale_active, ErrorCode::PresaleNotEnded);
    
    let stake_info = &mut ctx.accounts.stake_info;
    let global_state = &mut ctx.accounts.global_state;

    stake_info.amount = stake_info.amount.checked_add(amount).unwrap();
    global_state.total_staked = global_state.total_staked.checked_add(amount).unwrap();

    stake_info.start_time = Clock::get()?.unix_timestamp;
    stake_info.last_claim_time = Clock::get()?.unix_timestamp;

    token::transfer(ctx.accounts.stake_transfer_context(), amount)?;

    Ok(())
}
(ctx: Context<StakeTokens>, amount: u64) -> ProgramResult {
        let presale_state = &ctx.accounts.presale_state;
        require!(presale_state.is_presale_active, ErrorCode::PresaleNotEnded);
        
        let stake_info = &mut ctx.accounts.stake_info;
        stake_info.amount = stake_info.amount.checked_add(amount).unwrap();
        stake_info.start_time = Clock::get()?.unix_timestamp;
        stake_info.last_claim_time = Clock::get()?.unix_timestamp;
        
        token::transfer(ctx.accounts.stake_transfer_context(), amount)?;
        Ok(())
    }

    
pub fn unstake_tokens(ctx: Context<UnstakeTokens>) -> ProgramResult {
    let stake_info = &mut ctx.accounts.stake_info;
    let global_state = &mut ctx.accounts.global_state;

    let clock = Clock::get()?;
    let staking_duration = clock.unix_timestamp - stake_info.start_time;

    require!(staking_duration >= STAKING_DURATION, ErrorCode::FullStakingPeriodNotCompleted);

    let unstake_amount = stake_info.amount;
    require!(unstake_amount > 0, ErrorCode::InvalidAmount);

    global_state.total_staked = global_state.total_staked.checked_sub(unstake_amount).unwrap();
    stake_info.amount = 0;

    token::transfer(ctx.accounts.unstake_transfer_context(), unstake_amount)?;

    Ok(())
}
(ctx: Context<UnstakeTokens>) -> ProgramResult {
        let presale_state = &ctx.accounts.presale_state;
        require!(!presale_state.is_presale_active, ErrorCode::PresaleNotEnded);
        
        let clock = Clock::get()?;
        let duration = clock.unix_timestamp - presale_state.launch_time.unwrap();
        require!(duration >= EARLY_UNSTAKE_PERIOD, ErrorCode::UnstakingNotAllowedBefore7Days);
        
        let stake_info = &mut ctx.accounts.stake_info;
        let staking_duration = clock.unix_timestamp - stake_info.start_time;
        
        require!(staking_duration >= STAKING_DURATION, ErrorCode::FullStakingPeriodNotCompleted);
        
        token::transfer(ctx.accounts.unstake_transfer_context(), stake_info.amount)?;
        stake_info.amount = 0;
        
        Ok(())
    }

    pub fn lock_liquidity(ctx: Context<LockLiquidity>) -> ProgramResult {
        let clock = Clock::get()?;
        require!(clock.unix_timestamp + LIQUIDITY_LOCK_PERIOD > clock.unix_timestamp, ErrorCode::LiquidityLockError);
        Ok(())
    }
}


#[error]
pub enum ErrorCode {
    #[msg("Presale has not ended yet. Staking is not allowed.")]
    PresaleNotEnded,
    #[msg("Presale already ended.")]
    PresaleAlreadyEnded,
    #[msg("Unstaking not allowed before 7 days after launch.")]
    UnstakingNotAllowedBefore7Days,
    #[msg("Full staking period (6 months) not completed.")]
    FullStakingPeriodNotCompleted,
    #[msg("Liquidity lock error.")]
    LiquidityLockError,
    #[msg("Invalid payment amount.")]
    InvalidAmount,
    #[msg("Insufficient funds for SPL token transfer.")]
    InsufficientFunds,
    #[msg("Minimum stake amount not met.")]
    MinimumStakeNotMet,
    #[msg("Attempting to unstake more than staked balance.")]
    UnstakeAmountExceedsStake,
    #[msg("No rewards available to claim yet.")]
    NoRewardsAvailable,
}

pub enum ErrorCode {
    #[msg("Presale has not ended yet. Staking is not allowed.")]
    PresaleNotEnded,
    #[msg("Presale already ended.")]
    PresaleAlreadyEnded,
    #[msg("Unstaking not allowed before 7 days after launch.")]
    UnstakingNotAllowedBefore7Days,
    #[msg("Full staking period (6 months) not completed.")]
    FullStakingPeriodNotCompleted,
    #[msg("Liquidity lock error.")]
    LiquidityLockError,
    #[msg("Invalid payment amount.")]
    InvalidAmount,
    #[msg("Insufficient funds for SPL token transfer.")]
    InsufficientFunds,
}

#[derive(Accounts)]
pub struct AcceptPayment<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    #[account(mut)]
    pub token_account: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

#[account]
pub struct GlobalState {
    pub total_staked: u64,  // Total staked BRATS tokens across all users
}

pub fn claim_rewards(ctx: Context<ClaimRewards>) -> ProgramResult {
    let stake_info = &mut ctx.accounts.stake_info;
    let clock = Clock::get()?;
    
    let staking_time = clock.unix_timestamp - stake_info.last_claim_time;
    require!(staking_time > 0, ErrorCode::NoRewardsAvailable);

    let reward_amount = (stake_info.amount * APY * staking_time as u64) / (100 * STAKING_DURATION as u64);
    
    // Transfer rewards to user
    token::transfer(ctx.accounts.reward_transfer_context(), reward_amount)?;

    stake_info.last_claim_time = clock.unix_timestamp;

    Ok(())
}

pub fn calculate_rewards(ctx: Context<CalculateRewards>) -> Result<u64> {
    let stake_info = &ctx.accounts.stake_info;
    let clock = Clock::get()?;
    
    let staking_time = clock.unix_timestamp - stake_info.last_claim_time;
    require!(staking_time > 0, ErrorCode::NoRewardsAvailable);

    let reward_amount = (stake_info.amount * APY * staking_time as u64) / (100 * STAKING_DURATION as u64);

    Ok(reward_amount)
}
