// SPDX-License-Identifier: MIT
// $BRATS Smart Contract - Solana (Rust & Anchor Framework)

use anchor_lang::prelude::*;
use anchor_lang::solana_program;
use anchor_lang::solana_program::system_instruction;
use anchor_spl::token::{self, Burn, Mint, MintTo, Token, TokenAccount, Transfer};
use std::str::FromStr;

declare_id!("BxaA8XGHQG2z5X1J4JLcPVVdKBpzK3qSt1Bhk3YktW3s"); // Replace with your program ID

//
// CONSTANTS
//
const STAKING_DURATION: i64 = 180 * 24 * 3600; // 6 months in seconds
const EARLY_UNSTAKE_PERIOD: i64 = 7 * 24 * 3600; // 7-day lock after launch before early unstake is allowed
const LIQUIDITY_LOCK_PERIOD: i64 = 365 * 24 * 3600; // 1 year in seconds
const EARLY_UNSTAKE_PENALTY_PERCENT: u64 = 20; // 20% penalty for early unstake

// Our custom SPL token mint address (Devnet)
const CUSTOM_TOKEN_MINT: &str = "57EMXJXJkGYNCGjr9ngZPKnJr9jdJPZ1SRrWQqcxg9tr";

// Token metadata (for off‑chain display; integration with Metaplex is recommended)
const TOKEN_NAME: &str = "Brotherhood of Rats";
const TOKEN_SYMBOL: &str = "$BRATS";

// The fee wallet to receive fee portions (for both SOL and SPL tokens).
// All fees (a flat fee of 3) will be sent to this devnet wallet.
const FEE_WALLET: &str = "57EMXJXJkGYNCGjr9ngZPKnJr9jdJPZ1SRr9jdJPZ1SRr9tr";

//
// ACCOUNTS
//

#[account]
pub struct PresaleState {
    pub is_presale_active: bool,
    pub presale_end_time: Option<i64>,
    pub launch_time: Option<i64>,
    pub admin: Pubkey,
    pub liquidity_locked: bool,
    pub liquidity_lock_end_time: Option<i64>,
}

#[account]
pub struct GlobalState {
    pub total_staked: u64,            // Total staked $BRATS tokens across all users
    pub reward_pool: u64,             // Reward pool (in tokens) for stakers
    pub apy: u64,                     // Annual percentage yield (mutable via governance)
    pub transaction_fee_percent: u64, // Transaction fee percent (mutable via governance)
}

#[account]
pub struct StakeInfo {
    pub amount: u64,          // Amount of tokens staked
    pub start_time: i64,      // Timestamp when staking started
    pub last_claim_time: i64, // Timestamp of last reward claim
}

/// This account holds the presale stage data. There are 8 stages.
/// The `price` is stored as a fixed-point value with 8 decimals (e.g. 0.00021 is stored as 21000).
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy)]
pub struct PresaleStage {
    pub stage: u8,
    pub price: u64,
    pub tokens_sold: u64,
    pub total_raised: u64,
}

#[account]
pub struct PresaleStageInfo {
    pub stages: [PresaleStage; 8],
}

//
// PROGRAM
//
#[program]
pub mod brats_contract {
    use super::*;

    /// Initialize the presale state. Sets the admin to the specified devnet wallet.
    pub fn initialize_token(ctx: Context<InitializeToken>) -> ProgramResult {
        let presale_state = &mut ctx.accounts.presale_state;
        presale_state.is_presale_active = true;
        presale_state.presale_end_time = None;
        presale_state.launch_time = None;
        // Set the admin/owner to the specified devnet wallet
        presale_state.admin = Pubkey::from_str("57EMXJXJkGYNCGjr9ngZPKnJr9jdJPZ1SRr9jdJPZ1SRr9tr").unwrap();
        presale_state.liquidity_locked = false;
        presale_state.liquidity_lock_end_time = None;
        Ok(())
    }

    /// Initialize the global state with initial parameters.
    pub fn initialize_global_state(
        ctx: Context<InitializeGlobalState>,
        apy: u64,
        transaction_fee_percent: u64,
    ) -> ProgramResult {
        let global_state = &mut ctx.accounts.global_state;
        global_state.total_staked = 0;
        global_state.reward_pool = 0;
        global_state.apy = apy;
        global_state.transaction_fee_percent = transaction_fee_percent;
        Ok(())
    }

    /// End the presale and mark the launch time.
    /// After this, staking is disabled.
    pub fn end_presale(ctx: Context<EndPresale>) -> ProgramResult {
        let presale_state = &mut ctx.accounts.presale_state;
        require!(presale_state.is_presale_active, ErrorCode::PresaleAlreadyEnded);
        require!(
            ctx.accounts.admin.key() == presale_state.admin,
            ErrorCode::Unauthorized
        );
        let clock = Clock::get()?;
        presale_state.is_presale_active = false;
        presale_state.presale_end_time = Some(clock.unix_timestamp);
        presale_state.launch_time = Some(clock.unix_timestamp);
        presale_state.liquidity_lock_end_time =
            Some(clock.unix_timestamp + LIQUIDITY_LOCK_PERIOD);
        Ok(())
    }

    /// Accept payment in either SOL or our custom SPL token.
    /// A flat fee of 3 (units) is deducted and sent to the fee wallet.
    /// The remaining amount is transferred to the treasury.
    pub fn accept_payment(
        ctx: Context<AcceptPayment>,
        amount: u64,
        token_mint: Pubkey,
    ) -> ProgramResult {
        // Check that the fee wallet accounts are set to the correct devnet fee wallet.
        let fee_wallet_pubkey = Pubkey::from_str(FEE_WALLET).unwrap();
        require!(
            ctx.accounts.fee_wallet_sol_account.key == fee_wallet_pubkey,
            ErrorCode::InvalidFeeWallet
        );
        require!(
            ctx.accounts.fee_wallet_token_account.owner == fee_wallet_pubkey,
            ErrorCode::InvalidFeeWallet
        );

        if token_mint == Pubkey::default() {
            // SOL branch.
            // Ensure the amount is greater than the flat fee of 3.
            require!(amount > 3, ErrorCode::InvalidAmount);
            let fee = 3;
            let net_amount = amount.checked_sub(fee).unwrap();

            // Transfer net_amount from payer to treasury (SOL)
            let ix1 = system_instruction::transfer(
                &ctx.accounts.payer.key,
                ctx.accounts.treasury_sol_account.key,
                net_amount,
            );
            solana_program::program::invoke(
                &ix1,
                &[
                    ctx.accounts.payer.to_account_info(),
                    ctx.accounts.treasury_sol_account.clone(),
                    ctx.accounts.system_program.to_account_info(),
                ],
            )?;

            // Transfer fee from payer to fee wallet (SOL)
            let ix2 = system_instruction::transfer(
                &ctx.accounts.payer.key,
                ctx.accounts.fee_wallet_sol_account.key,
                fee,
            );
            solana_program::program::invoke(
                &ix2,
                &[
                    ctx.accounts.payer.to_account_info(),
                    ctx.accounts.fee_wallet_sol_account.clone(),
                    ctx.accounts.system_program.to_account_info(),
                ],
            )?;
        } else if token_mint == Pubkey::from_str(CUSTOM_TOKEN_MINT).unwrap() {
            // SPL branch for our custom token.
            require!(
                ctx.accounts.payer_token_account.amount >= amount,
                ErrorCode::InsufficientFunds
            );
            let fee = 3;
            let net_amount = amount.checked_sub(fee).unwrap();

            // Transfer net_amount from payer to treasury (SPL)
            token::transfer(
                ctx.accounts.stake_transfer_context_generic(
                    ctx.accounts.payer_token_account.to_account_info(),
                    ctx.accounts.treasury_token_account.to_account_info(),
                ),
                net_amount,
            )?;
            // Transfer fee from payer to fee wallet (SPL)
            token::transfer(
                ctx.accounts.stake_transfer_context_generic(
                    ctx.accounts.payer_token_account.to_account_info(),
                    ctx.accounts.fee_wallet_token_account.to_account_info(),
                ),
                fee,
            )?;
        } else {
            return Err(ErrorCode::InvalidTokenMint.into());
        }
        Ok(())
    }

    /// Deposit SOL into the treasury.
    /// This is a dedicated deposit instruction for SOL.
    pub fn deposit_sol(ctx: Context<DepositSol>, amount: u64) -> ProgramResult {
        let ix = system_instruction::transfer(
            &ctx.accounts.payer.key,
            ctx.accounts.treasury_sol_account.key,
            amount,
        );
        solana_program::program::invoke(
            &ix,
            &[
                ctx.accounts.payer.to_account_info(),
                ctx.accounts.treasury_sol_account.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
        )?;
        Ok(())
    }

    /// Stake tokens during the presale.
    /// Staking is allowed only while the presale is active and if rewards are available.
    pub fn stake_tokens(ctx: Context<StakeTokens>, amount: u64) -> ProgramResult {
        // Allow staking only if presale is active.
        require!(
            ctx.accounts.presale_state.is_presale_active,
            ErrorCode::StakingClosed
        );
        // Also, ensure the reward pool is not empty.
        require!(
            ctx.accounts.global_state.reward_pool > 0,
            ErrorCode::StakingRewardsExhausted
        );
        require!(amount > 0, ErrorCode::InvalidAmount);

        let stake_info = &mut ctx.accounts.stake_info;
        let global_state = &mut ctx.accounts.global_state;
        stake_info.amount = stake_info.amount.checked_add(amount).unwrap();
        global_state.total_staked = global_state.total_staked.checked_add(amount).unwrap();
        let clock = Clock::get()?;
        stake_info.start_time = clock.unix_timestamp;
        stake_info.last_claim_time = clock.unix_timestamp;

        // Transfer tokens from the user's account to the staking pool.
        token::transfer(
            ctx.accounts.stake_transfer_context(),
            amount,
        )?;
        Ok(())
    }

    /// Unstake tokens.
    /// If the full staking duration has been met, the full stake is returned.
    /// Otherwise, if early unstaking is used (allowed only after 7 days from launch),
    /// a 20% penalty is applied: the user receives (100 - penalty)% of their staked tokens
    /// and the penalty portion is burned.
    pub fn unstake_tokens(ctx: Context<UnstakeTokens>) -> ProgramResult {
        let stake_info = &mut ctx.accounts.stake_info;
        let global_state = &mut ctx.accounts.global_state;
        let clock = Clock::get()?;
        let staking_duration = clock.unix_timestamp - stake_info.start_time;

        // Check that early unstaking is allowed (7 days after launch)
        if let Some(launch_time) = ctx.accounts.presale_state.launch_time {
            if clock.unix_timestamp < launch_time + EARLY_UNSTAKE_PERIOD {
                return Err(ErrorCode::UnstakingNotAllowedBefore7Days.into());
            }
        }

        require!(stake_info.amount > 0, ErrorCode::InvalidAmount);
        if staking_duration >= STAKING_DURATION {
            // Full staking period complete: return full staked amount.
            let unstake_amount = stake_info.amount;
            global_state.total_staked = global_state.total_staked.checked_sub(unstake_amount).unwrap();
            stake_info.amount = 0;
            token::transfer(ctx.accounts.unstake_transfer_context(), unstake_amount)?;
        } else {
            // Early unstake: apply penalty.
            let penalty_amount = stake_info
                .amount
                .checked_mul(EARLY_UNSTAKE_PENALTY_PERCENT)
                .unwrap()
                .checked_div(100)
                .unwrap();
            let unstake_amount = stake_info.amount.checked_sub(penalty_amount).unwrap();
            global_state.total_staked = global_state.total_staked.checked_sub(stake_info.amount).unwrap();
            stake_info.amount = 0;
            // Return the remaining tokens to the user.
            token::transfer(ctx.accounts.unstake_transfer_context(), unstake_amount)?;
            // Burn the penalty tokens.
            token::burn(ctx.accounts.early_unstake_burn_context(), penalty_amount)?;
        }
        Ok(())
    }

    /// Lock liquidity by transferring liquidity tokens to a vault.
    /// This function should be called (by admin or automatically) while liquidity is still locked.
    pub fn lock_liquidity(ctx: Context<LockLiquidity>) -> ProgramResult {
        let clock = Clock::get()?;
        let presale_state = &mut ctx.accounts.presale_state;
        if let Some(lock_end) = presale_state.liquidity_lock_end_time {
            if clock.unix_timestamp < lock_end {
                let amount = ctx.accounts.liquidity_token_account.amount;
                require!(amount > 0, ErrorCode::InvalidAmount);
                token::transfer(
                    ctx.accounts.liquidity_lock_transfer_context(),
                    amount,
                )?;
                presale_state.liquidity_locked = true;
                return Ok(());
            }
        }
        Err(ErrorCode::LiquidityLockError.into())
    }

    /// Claim staking rewards.
    /// Rewards are calculated based on the staked amount, the time since the last claim,
    /// and the current APY stored in GlobalState.
    pub fn claim_rewards(ctx: Context<ClaimRewards>) -> ProgramResult {
        let stake_info = &mut ctx.accounts.stake_info;
        let global_state = &mut ctx.accounts.global_state;
        let clock = Clock::get()?;
        let staking_time = clock.unix_timestamp - stake_info.last_claim_time;
        require!(staking_time > 0, ErrorCode::NoRewardsAvailable);

        let reward_amount = (stake_info.amount)
            .checked_mul(global_state.apy)
            .unwrap()
            .checked_mul(staking_time as u64)
            .unwrap()
            .checked_div(100 * STAKING_DURATION as u64)
            .unwrap();

        require!(
            ctx.accounts.reward_pool_token_account.amount >= reward_amount,
            ErrorCode::InsufficientRewards
        );

        global_state.reward_pool = global_state.reward_pool.checked_sub(reward_amount).unwrap();
        token::transfer(ctx.accounts.reward_transfer_context(), reward_amount)?;
        stake_info.last_claim_time = clock.unix_timestamp;
        Ok(())
    }

    /// Calculate rewards for display (off‑chain) without transferring tokens.
    pub fn calculate_rewards(ctx: Context<CalculateRewards>) -> Result<u64> {
        let stake_info = &ctx.accounts.stake_info;
        let clock = Clock::get()?;
        let staking_time = clock.unix_timestamp - stake_info.last_claim_time;
        require!(staking_time > 0, ErrorCode::NoRewardsAvailable);
        let reward_amount = (stake_info.amount)
            .checked_mul(ctx.accounts.global_state.apy)
            .unwrap()
            .checked_mul(staking_time as u64)
            .unwrap()
            .checked_div(100 * STAKING_DURATION as u64)
            .unwrap();
        Ok(reward_amount)
    }

    /// Burn tokens from a source account. (Admin only)
    pub fn burn_tokens(ctx: Context<BurnTokens>, amount: u64) -> ProgramResult {
        require!(
            ctx.accounts.admin.key() == ctx.accounts.presale_state.admin,
            ErrorCode::Unauthorized
        );
        token::burn(ctx.accounts.burn_context(), amount)?;
        Ok(())
    }

    /// Refill the reward pool by transferring tokens into the reward pool account. (Admin only)
    pub fn refill_reward_pool(ctx: Context<RefillRewardPool>, amount: u64) -> ProgramResult {
        require!(
            ctx.accounts.admin.key() == ctx.accounts.presale_state.admin,
            ErrorCode::Unauthorized
        );
        token::transfer(ctx.accounts.refill_transfer_context(), amount)?;
        ctx.accounts.global_state.reward_pool = ctx
            .accounts
            .global_state
            .reward_pool
            .checked_add(amount)
            .unwrap();
        Ok(())
    }

    /// Update APY and transaction fee percent. (Admin only)
    pub fn update_parameters(
        ctx: Context<UpdateParameters>,
        new_apy: u64,
        new_fee_percent: u64,
    ) -> ProgramResult {
        require!(
            ctx.accounts.admin.key() == ctx.accounts.presale_state.admin,
            ErrorCode::Unauthorized
        );
        let global_state = &mut ctx.accounts.global_state;
        global_state.apy = new_apy;
        global_state.transaction_fee_percent = new_fee_percent;
        Ok(())
    }

    /// Allow the admin to withdraw funds from the treasury SOL account during the presale.
    pub fn withdraw_funds(ctx: Context<WithdrawFunds>, amount: u64) -> ProgramResult {
        // Only allow withdrawal while presale is active.
        require!(
            ctx.accounts.presale_state.is_presale_active,
            ErrorCode::WithdrawalNotAllowedAfterPresale
        );
        let ix = system_instruction::transfer(
            ctx.accounts.treasury_sol_account.key,
            ctx.accounts.admin.key,
            amount,
        );
        solana_program::program::invoke(
            &ix,
            &[
                ctx.accounts.treasury_sol_account.clone(),
                ctx.accounts.admin.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
        )?;
        Ok(())
    }

    /// Initialize the presale stage information with default stages.
    pub fn initialize_presale_stages(ctx: Context<InitializePresaleStages>) -> ProgramResult {
        let presale_stage_info = &mut ctx.accounts.presale_stage_info;
        presale_stage_info.stages = [
            // Prices are stored with 8 decimals (e.g. 0.00021 -> 21000)
            PresaleStage { stage: 1, price: 21000, tokens_sold: 2_500_000_000, total_raised: 525_000 },
            PresaleStage { stage: 2, price: 25000, tokens_sold: 2_500_000_000, total_raised: 625_000 },
            PresaleStage { stage: 3, price: 29000, tokens_sold: 2_500_000_000, total_raised: 725_000 },
            PresaleStage { stage: 4, price: 33000, tokens_sold: 2_500_000_000, total_raised: 825_000 },
            PresaleStage { stage: 5, price: 37000, tokens_sold: 2_500_000_000, total_raised: 925_000 },
            PresaleStage { stage: 6, price: 41000, tokens_sold: 2_500_000_000, total_raised: 1_025_000 },
            PresaleStage { stage: 7, price: 45000, tokens_sold: 2_500_000_000, total_raised: 1_125_000 },
            PresaleStage { stage: 8, price: 49000, tokens_sold: 2_500_000_000, total_raised: 1_225_000 },
        ];
        Ok(())
    }

    /// Update a specific presale stage (Admin only).
    /// `stage_index` is 0-based (i.e. 0 for Stage 1, 1 for Stage 2, etc.)
    pub fn update_presale_stage(
        ctx: Context<UpdatePresaleStage>,
        stage_index: u8,
        price: u64,
        tokens_sold: u64,
        total_raised: u64,
    ) -> ProgramResult {
        let presale_stage_info = &mut ctx.accounts.presale_stage_info;
        require!(
            (stage_index as usize) < presale_stage_info.stages.len(),
            ErrorCode::InvalidStageIndex
        );
        presale_stage_info.stages[stage_index as usize] = PresaleStage {
            stage: stage_index + 1,
            price,
            tokens_sold,
            total_raised,
        };
        Ok(())
    }
}

//
// ERROR CODES
//
#[error]
pub enum ErrorCode {
    #[msg("Presale has not ended yet. Staking is only allowed during the presale.")]
    PresaleNotEnded,
    #[msg("Presale already ended.")]
    PresaleAlreadyEnded,
    #[msg("Unstaking not allowed before 7 days after launch.")]
    UnstakingNotAllowedBefore7Days,
    #[msg("Liquidity lock error.")]
    LiquidityLockError,
    #[msg("Invalid payment or stake amount.")]
    InvalidAmount,
    #[msg("Insufficient funds for SPL token transfer.")]
    InsufficientFunds,
    #[msg("No rewards available to claim yet.")]
    NoRewardsAvailable,
    #[msg("Invalid token mint address.")]
    InvalidTokenMint,
    #[msg("Not enough rewards in the pool.")]
    InsufficientRewards,
    #[msg("Unauthorized.")]
    Unauthorized,
    #[msg("Fee wallet provided is invalid.")]
    InvalidFeeWallet,
    #[msg("Staking is only allowed during the presale.")]
    StakingClosed,
    #[msg("Staking rewards pool is exhausted.")]
    StakingRewardsExhausted,
    #[msg("Withdrawal allowed only during presale.")]
    WithdrawalNotAllowedAfterPresale,
    #[msg("Invalid presale stage index.")]
    InvalidStageIndex,
}

//
// CONTEXTS & HELPER FUNCTIONS
//

// ---------- InitializeToken ----------
#[derive(Accounts)]
pub struct InitializeToken<'info> {
    #[account(init, payer = payer, space = 8 + std::mem::size_of::<PresaleState>())]
    pub presale_state: Account<'info, PresaleState>,
    #[account(mut)]
    pub payer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

// ---------- InitializeGlobalState ----------
#[derive(Accounts)]
pub struct InitializeGlobalState<'info> {
    #[account(init, payer = payer, space = 8 + std::mem::size_of::<GlobalState>())]
    pub global_state: Account<'info, GlobalState>,
    #[account(mut)]
    pub payer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

// ---------- EndPresale ----------
#[derive(Accounts)]
pub struct EndPresale<'info> {
    #[account(mut)]
    pub presale_state: Account<'info, PresaleState>,
    #[account(mut)]
    pub admin: Signer<'info>,
}

// ---------- AcceptPayment ----------
/// This context includes accounts for both SOL and SPL branches.
/// (Unused accounts for one branch can be ignored.)
#[derive(Accounts)]
pub struct AcceptPayment<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    // SPL token accounts
    #[account(mut)]
    pub payer_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub treasury_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub fee_wallet_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub reward_pool_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub mint: Account<'info, Mint>,
    pub mint_authority: Signer<'info>,

    // SOL accounts (for SOL payments)
    /// CHECK: Treasury SOL account (must be a non‑executable wallet)
    #[account(mut)]
    pub treasury_sol_account: AccountInfo<'info>,
    /// CHECK: Fee wallet SOL account (must be a non‑executable wallet)
    #[account(mut)]
    pub fee_wallet_sol_account: AccountInfo<'info>,
    /// CHECK: Reward pool SOL account
    #[account(mut)]
    pub reward_pool_sol_account: AccountInfo<'info>,

    // Global state (holds fee parameters and reward pool tracker)
    #[account(mut)]
    pub global_state: Account<'info, GlobalState>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

impl<'info> AcceptPayment<'info> {
    /// A generic transfer context used for SPL token transfers.
    pub fn stake_transfer_context_generic(
        &self,
        from: AccountInfo<'info>,
        to: AccountInfo<'info>,
    ) -> CpiContext<'_, '_, '_, 'info, Transfer<'info>> {
        let cpi_accounts = Transfer {
            from,
            to,
            authority: self.payer.to_account_info(),
        };
        CpiContext::new(self.token_program.to_account_info(), cpi_accounts)
    }
}

// ---------- DepositSol ----------
#[derive(Accounts)]
pub struct DepositSol<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    /// CHECK: Treasury SOL account where the deposit will be transferred.
    #[account(mut)]
    pub treasury_sol_account: AccountInfo<'info>,
    pub system_program: Program<'info, System>,
}

// ---------- StakeTokens ----------
#[derive(Accounts)]
pub struct StakeTokens<'info> {
    #[account(mut)]
    pub stake_info: Account<'info, StakeInfo>,
    #[account(mut)]
    pub global_state: Account<'info, GlobalState>,
    #[account(mut)]
    pub presale_state: Account<'info, PresaleState>,
    #[account(mut)]
    pub payer: Signer<'info>,
    /// The user's token account (source).
    #[account(mut)]
    pub user_token_account: Account<'info, TokenAccount>,
    /// The staking pool token account (destination).
    #[account(mut)]
    pub staking_pool_token_account: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

impl<'info> StakeTokens<'info> {
    /// Returns a CPI context for transferring tokens from the user to the staking pool.
    pub fn stake_transfer_context(
        &self,
    ) -> CpiContext<'_, '_, '_, 'info, Transfer<'info>> {
        let cpi_accounts = Transfer {
            from: self.user_token_account.to_account_info(),
            to: self.staking_pool_token_account.to_account_info(),
            authority: self.payer.to_account_info(),
        };
        CpiContext::new(self.token_program.to_account_info(), cpi_accounts)
    }
}

// ---------- UnstakeTokens ----------
#[derive(Accounts)]
pub struct UnstakeTokens<'info> {
    #[account(mut)]
    pub stake_info: Account<'info, StakeInfo>,
    #[account(mut)]
    pub global_state: Account<'info, GlobalState>,
    #[account(mut)]
    pub presale_state: Account<'info, PresaleState>,
    #[account(mut)]
    pub payer: Signer<'info>,
    /// The staking pool token account (source for unstake and burn).
    #[account(mut)]
    pub staking_pool_token_account: Account<'info, TokenAccount>,
    /// The user's token account (destination for unstaked tokens).
    #[account(mut)]
    pub user_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub mint: Account<'info, Mint>,
    pub token_program: Program<'info, Token>,
}

impl<'info> UnstakeTokens<'info> {
    /// Returns a CPI context for transferring tokens from the staking pool back to the user.
    pub fn unstake_transfer_context(
        &self,
    ) -> CpiContext<'_, '_, '_, 'info, Transfer<'info>> {
        let cpi_accounts = Transfer {
            from: self.staking_pool_token_account.to_account_info(),
            to: self.user_token_account.to_account_info(),
            authority: self.payer.to_account_info(),
        };
        CpiContext::new(self.token_program.to_account_info(), cpi_accounts)
    }
    /// Returns a CPI context for burning tokens from the staking pool (penalty).
    pub fn early_unstake_burn_context(
        &self,
    ) -> CpiContext<'_, '_, '_, 'info, Burn<'info>> {
        let cpi_accounts = Burn {
            mint: self.mint.to_account_info(),
            to: self.staking_pool_token_account.to_account_info(),
            authority: self.payer.to_account_info(),
        };
        CpiContext::new(self.token_program.to_account_info(), cpi_accounts)
    }
}

// ---------- ClaimRewards ----------
#[derive(Accounts)]
pub struct ClaimRewards<'info> {
    #[account(mut)]
    pub stake_info: Account<'info, StakeInfo>,
    #[account(mut)]
    pub global_state: Account<'info, GlobalState>,
    #[account(mut)]
    pub payer: Signer<'info>,
    /// The user's token account that will receive reward tokens.
    #[account(mut)]
    pub user_token_account: Account<'info, TokenAccount>,
    /// The reward pool token account (source).
    #[account(mut)]
    pub reward_pool_token_account: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

impl<'info> ClaimRewards<'info> {
    /// Returns a CPI context for transferring reward tokens from the reward pool to the user.
    pub fn reward_transfer_context(
        &self,
    ) -> CpiContext<'_, '_, '_, 'info, Transfer<'info>> {
        let cpi_accounts = Transfer {
            from: self.reward_pool_token_account.to_account_info(),
            to: self.user_token_account.to_account_info(),
            authority: self.payer.to_account_info(),
        };
        CpiContext::new(self.token_program.to_account_info(), cpi_accounts)
    }
}

// ---------- CalculateRewards ----------
#[derive(Accounts)]
pub struct CalculateRewards<'info> {
    #[account(mut)]
    pub stake_info: Account<'info, StakeInfo>,
    #[account(mut)]
    pub global_state: Account<'info, GlobalState>,
    pub payer: Signer<'info>,
    pub token_program: Program<'info, Token>,
}

// ---------- LockLiquidity ----------
#[derive(Accounts)]
pub struct LockLiquidity<'info> {
    #[account(mut)]
    pub presale_state: Account<'info, PresaleState>,
    /// The token account holding liquidity tokens to be locked.
    #[account(mut)]
    pub liquidity_token_account: Account<'info, TokenAccount>,
    /// The vault token account where liquidity tokens will be stored.
    #[account(mut)]
    pub vault_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub payer: Signer<'info>,
    pub token_program: Program<'info, Token>,
}

impl<'info> LockLiquidity<'info> {
    /// Returns a CPI context for transferring liquidity tokens into the vault.
    pub fn liquidity_lock_transfer_context(
        &self,
    ) -> CpiContext<'_, '_, '_, 'info, Transfer<'info>> {
        let cpi_accounts = Transfer {
            from: self.liquidity_token_account.to_account_info(),
            to: self.vault_account.to_account_info(),
            authority: self.payer.to_account_info(),
        };
        CpiContext::new(self.token_program.to_account_info(), cpi_accounts)
    }
}

// ---------- BurnTokens ----------
#[derive(Accounts)]
pub struct BurnTokens<'info> {
    #[account(mut)]
    pub presale_state: Account<'info, PresaleState>,
    #[account(mut)]
    pub mint: Account<'info, Mint>,
    /// The source token account from which tokens will be burned.
    #[account(mut)]
    pub source: Account<'info, TokenAccount>,
    pub admin: Signer<'info>,
    pub token_program: Program<'info, Token>,
}

impl<'info> BurnTokens<'info> {
    pub fn burn_context(
        &self,
    ) -> CpiContext<'_, '_, '_, 'info, Burn<'info>> {
        let cpi_accounts = Burn {
            mint: self.mint.to_account_info(),
            to: self.source.to_account_info(),
            authority: self.admin.to_account_info(),
        };
        CpiContext::new(self.token_program.to_account_info(), cpi_accounts)
    }
}

// ---------- RefillRewardPool ----------
#[derive(Accounts)]
pub struct RefillRewardPool<'info> {
    #[account(mut)]
    pub presale_state: Account<'info, PresaleState>,
    #[account(mut)]
    pub global_state: Account<'info, GlobalState>,
    /// The source token account (admin’s account) from which tokens will be transferred.
    #[account(mut)]
    pub source: Account<'info, TokenAccount>,
    /// The reward pool token account to be refilled.
    #[account(mut)]
    pub reward_pool_token_account: Account<'info, TokenAccount>,
    pub admin: Signer<'info>,
    pub token_program: Program<'info, Token>,
}

impl<'info> RefillRewardPool<'info> {
    pub fn refill_transfer_context(
        &self,
    ) -> CpiContext<'_, '_, '_, 'info, Transfer<'info>> {
        let cpi_accounts = Transfer {
            from: self.source.to_account_info(),
            to: self.reward_pool_token_account.to_account_info(),
            authority: self.admin.to_account_info(),
        };
        CpiContext::new(self.token_program.to_account_info(), cpi_accounts)
    }
}

// ---------- UpdateParameters ----------
#[derive(Accounts)]
pub struct UpdateParameters<'info> {
    #[account(mut)]
    pub presale_state: Account<'info, PresaleState>,
    #[account(mut)]
    pub global_state: Account<'info, GlobalState>,
    pub admin: Signer<'info>,
}

// ---------- WithdrawFunds ----------
#[derive(Accounts)]
pub struct WithdrawFunds<'info> {
    #[account(mut)]
    pub presale_state: Account<'info, PresaleState>,
    /// CHECK: Treasury SOL account from which funds will be withdrawn.
    #[account(mut)]
    pub treasury_sol_account: AccountInfo<'info>,
    #[account(mut)]
    pub admin: Signer<'info>,
    pub system_program: Program<'info, System>,
}

// ---------- InitializePresaleStages ----------
#[derive(Accounts)]
pub struct InitializePresaleStages<'info> {
    #[account(init, payer = payer, space = 8 + std::mem::size_of::<PresaleStageInfo>())]
    pub presale_stage_info: Account<'info, PresaleStageInfo>,
    #[account(mut)]
    pub payer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

// ---------- UpdatePresaleStage ----------
#[derive(Accounts)]
pub struct UpdatePresaleStage<'info> {
    #[account(mut)]
    pub presale_stage_info: Account<'info, PresaleStageInfo>,
    pub admin: Signer<'info>,
}
