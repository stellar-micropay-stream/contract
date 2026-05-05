#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Env, Symbol};

// ── Storage keys ──────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Escrow(Symbol), // keyed by escrow_id
}

// ── State ─────────────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone)]
pub struct EscrowState {
    pub sender: Address,
    pub receiver: Address,
    pub token: Address,
    pub amount: i128,
    pub unlock_time: u64, // ledger timestamp; 0 = no lock
    pub released: bool,
}

// ── Contract ──────────────────────────────────────────────────────────────────

#[contract]
pub struct EscrowContract;

#[contractimpl]
impl EscrowContract {
    /// Sender deposits `amount` tokens into escrow.
    /// `unlock_time` is a Unix timestamp; pass 0 for no time-lock.
    pub fn deposit(
        env: Env,
        escrow_id: Symbol,
        sender: Address,
        receiver: Address,
        token: Address,
        amount: i128,
        unlock_time: u64,
    ) {
        sender.require_auth();
        assert!(amount > 0, "amount must be positive");

        let key = DataKey::Escrow(escrow_id);
        assert!(
            !env.storage().persistent().has(&key),
            "escrow already exists"
        );

        // Pull tokens from sender into this contract
        token::Client::new(&env, &token).transfer(
            &sender,
            &env.current_contract_address(),
            &amount,
        );

        env.storage().persistent().set(
            &key,
            &EscrowState {
                sender,
                receiver,
                token,
                amount,
                unlock_time,
                released: false,
            },
        );
    }

    /// Receiver (or sender after time-lock) claims the escrowed funds.
    pub fn release(env: Env, escrow_id: Symbol, caller: Address) {
        caller.require_auth();

        let key = DataKey::Escrow(escrow_id);
        let state: EscrowState = env.storage().persistent().get(&key).expect("not found");

        assert!(!state.released, "already released");

        let now = env.ledger().timestamp();

        // Receiver can always release; sender can release only after time-lock
        if caller == state.receiver {
            // ok
        } else if caller == state.sender {
            assert!(
                state.unlock_time == 0 || now >= state.unlock_time,
                "time-lock active"
            );
        } else {
            panic!("unauthorized");
        }

        token::Client::new(&env, &state.token).transfer(
            &env.current_contract_address(),
            &state.receiver,
            &state.amount,
        );

        env.storage().persistent().set(
            &key,
            &EscrowState {
                released: true,
                ..state
            },
        );
    }

    /// Sender reclaims funds — only allowed after the time-lock expires.
    pub fn refund(env: Env, escrow_id: Symbol) {
        let key = DataKey::Escrow(escrow_id);
        let state: EscrowState = env.storage().persistent().get(&key).expect("not found");

        assert!(!state.released, "already released");
        assert!(state.unlock_time > 0, "no time-lock set");

        let now = env.ledger().timestamp();
        assert!(now >= state.unlock_time, "time-lock active");

        state.sender.require_auth();

        token::Client::new(&env, &state.token).transfer(
            &env.current_contract_address(),
            &state.sender,
            &state.amount,
        );

        env.storage().persistent().set(
            &key,
            &EscrowState {
                released: true,
                ..state
            },
        );
    }

    /// Read escrow state (view).
    pub fn get_escrow(env: Env, escrow_id: Symbol) -> EscrowState {
        let key = DataKey::Escrow(escrow_id);
        env.storage().persistent().get(&key).expect("not found")
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{
        symbol_short,
        testutils::{Address as _, Ledger},
        token::{Client as TokenClient, StellarAssetClient},
        Env,
    };

    fn setup() -> (Env, EscrowContractClient<'static>, Address, Address, Address) {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register(EscrowContract, ());
        let client = EscrowContractClient::new(&env, &contract_id);

        let sender = Address::generate(&env);
        let receiver = Address::generate(&env);

        // Deploy a test token and mint to sender
        let token_admin = Address::generate(&env);
        let token_id = env.register_stellar_asset_contract_v2(token_admin.clone());
        let token_addr = token_id.address();
        StellarAssetClient::new(&env, &token_addr).mint(&sender, &1_000_000);

        (env, client, sender, receiver, token_addr)
    }

    #[test]
    fn test_deposit_and_release_by_receiver() {
        let (env, client, sender, receiver, token) = setup();
        let id = symbol_short!("esc1");

        client.deposit(&id, &sender, &receiver, &token, &500_000, &0);

        let state = client.get_escrow(&id);
        assert_eq!(state.amount, 500_000);
        assert!(!state.released);

        client.release(&id, &receiver);

        let state = client.get_escrow(&id);
        assert!(state.released);

        // Receiver balance should be 500_000
        assert_eq!(TokenClient::new(&env, &token).balance(&receiver), 500_000);
    }

    #[test]
    fn test_refund_after_timelock() {
        let (env, client, sender, receiver, token) = setup();
        let id = symbol_short!("esc2");

        let unlock = 1_000u64;
        client.deposit(&id, &sender, &receiver, &token, &300_000, &unlock);

        // Advance ledger past unlock time
        env.ledger().with_mut(|l| l.timestamp = 2_000);

        client.refund(&id);

        let state = client.get_escrow(&id);
        assert!(state.released);
        assert_eq!(TokenClient::new(&env, &token).balance(&sender), 1_000_000);
    }

    #[test]
    #[should_panic(expected = "time-lock active")]
    fn test_refund_before_timelock_panics() {
        let (_env, client, sender, receiver, token) = setup();
        let id = symbol_short!("esc3");

        client.deposit(&id, &sender, &receiver, &token, &100_000, &9_999_999);
        client.refund(&id); // should panic
    }

    #[test]
    #[should_panic(expected = "already released")]
    fn test_double_release_panics() {
        let (_env, client, sender, receiver, token) = setup();
        let id = symbol_short!("esc4");

        client.deposit(&id, &sender, &receiver, &token, &100_000, &0);
        client.release(&id, &receiver);
        client.release(&id, &receiver); // should panic
    }
}
