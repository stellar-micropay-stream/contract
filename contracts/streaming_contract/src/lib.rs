#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Env, Symbol};

// ── Storage keys ──────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Stream(Symbol),
}

// ── State ─────────────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone)]
pub struct StreamState {
    pub sender: Address,
    pub receiver: Address,
    pub token: Address,
    /// Tokens per second (scaled: 1 unit = 1 stroop = 0.0000001 XLM)
    pub rate_per_sec: i128,
    /// Total deposited by sender
    pub deposit: i128,
    /// Total already transferred to receiver
    pub transferred: i128,
    /// Ledger timestamp when stream was opened / last ticked
    pub last_tick: u64,
    pub closed: bool,
}

// ── Contract ──────────────────────────────────────────────────────────────────

#[contract]
pub struct StreamingContract;

#[contractimpl]
impl StreamingContract {
    /// Open a new payment stream. Sender deposits tokens upfront.
    /// `rate_per_sec`: tokens (stroops) to transfer per second.
    pub fn open_stream(
        env: Env,
        stream_id: Symbol,
        sender: Address,
        receiver: Address,
        token: Address,
        deposit: i128,
        rate_per_sec: i128,
    ) {
        sender.require_auth();
        assert!(deposit > 0, "deposit must be positive");
        assert!(rate_per_sec > 0, "rate must be positive");

        let key = DataKey::Stream(stream_id);
        assert!(
            !env.storage().persistent().has(&key),
            "stream already exists"
        );

        token::Client::new(&env, &token).transfer(
            &sender,
            &env.current_contract_address(),
            &deposit,
        );

        env.storage().persistent().set(
            &key,
            &StreamState {
                sender,
                receiver,
                token,
                rate_per_sec,
                deposit,
                transferred: 0,
                last_tick: env.ledger().timestamp(),
                closed: false,
            },
        );
    }

    /// Settle elapsed payment to receiver based on time passed since last tick.
    /// Anyone can call tick; it is idempotent if no time has elapsed.
    pub fn tick(env: Env, stream_id: Symbol) {
        let key = DataKey::Stream(stream_id);
        let mut state: StreamState =
            env.storage().persistent().get(&key).expect("not found");

        assert!(!state.closed, "stream closed");

        let now = env.ledger().timestamp();
        let elapsed = now.saturating_sub(state.last_tick) as i128;

        if elapsed == 0 {
            return;
        }

        let remaining = state.deposit - state.transferred;
        let due = (state.rate_per_sec * elapsed).min(remaining);

        if due > 0 {
            token::Client::new(&env, &state.token).transfer(
                &env.current_contract_address(),
                &state.receiver,
                &due,
            );
            state.transferred += due;
        }

        state.last_tick = now;
        env.storage().persistent().set(&key, &state);
    }

    /// Close the stream: settle remaining owed to receiver, refund leftover to sender.
    pub fn close_stream(env: Env, stream_id: Symbol, caller: Address) {
        caller.require_auth();

        let key = DataKey::Stream(stream_id);
        let mut state: StreamState =
            env.storage().persistent().get(&key).expect("not found");

        assert!(!state.closed, "already closed");
        assert!(
            caller == state.sender || caller == state.receiver,
            "unauthorized"
        );

        // Settle up to now
        let now = env.ledger().timestamp();
        let elapsed = now.saturating_sub(state.last_tick) as i128;
        let remaining = state.deposit - state.transferred;
        let due = (state.rate_per_sec * elapsed).min(remaining);

        let token = token::Client::new(&env, &state.token);

        if due > 0 {
            token.transfer(&env.current_contract_address(), &state.receiver, &due);
            state.transferred += due;
        }

        // Refund unspent deposit to sender
        let leftover = state.deposit - state.transferred;
        if leftover > 0 {
            token.transfer(&env.current_contract_address(), &state.sender, &leftover);
        }

        state.closed = true;
        state.last_tick = now;
        env.storage().persistent().set(&key, &state);
    }

    /// View stream state.
    pub fn get_stream(env: Env, stream_id: Symbol) -> StreamState {
        let key = DataKey::Stream(stream_id);
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

    fn setup() -> (
        Env,
        StreamingContractClient<'static>,
        Address,
        Address,
        Address,
    ) {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register(StreamingContract, ());
        let client = StreamingContractClient::new(&env, &contract_id);

        let sender = Address::generate(&env);
        let receiver = Address::generate(&env);

        let token_admin = Address::generate(&env);
        let token_id = env.register_stellar_asset_contract_v2(token_admin.clone());
        let token_addr = token_id.address();
        StellarAssetClient::new(&env, &token_addr).mint(&sender, &1_000_000);

        (env, client, sender, receiver, token_addr)
    }

    #[test]
    fn test_open_and_tick() {
        let (env, client, sender, receiver, token) = setup();
        let id = symbol_short!("s1");

        // rate = 100 stroops/sec, deposit = 10_000
        env.ledger().with_mut(|l| l.timestamp = 1_000);
        client.open_stream(&id, &sender, &receiver, &token, &10_000, &100);

        // Advance 50 seconds → 5_000 due
        env.ledger().with_mut(|l| l.timestamp = 1_050);
        client.tick(&id);

        assert_eq!(TokenClient::new(&env, &token).balance(&receiver), 5_000);

        let state = client.get_stream(&id);
        assert_eq!(state.transferred, 5_000);
        assert_eq!(state.last_tick, 1_050);
    }

    #[test]
    fn test_tick_caps_at_deposit() {
        let (env, client, sender, receiver, token) = setup();
        let id = symbol_short!("s2");

        env.ledger().with_mut(|l| l.timestamp = 0);
        client.open_stream(&id, &sender, &receiver, &token, &500, &100);

        // Advance 100 seconds → would owe 10_000 but deposit is only 500
        env.ledger().with_mut(|l| l.timestamp = 100);
        client.tick(&id);

        assert_eq!(TokenClient::new(&env, &token).balance(&receiver), 500);
    }

    #[test]
    fn test_close_stream_refunds_leftover() {
        let (env, client, sender, receiver, token) = setup();
        let id = symbol_short!("s3");

        env.ledger().with_mut(|l| l.timestamp = 0);
        client.open_stream(&id, &sender, &receiver, &token, &10_000, &100);

        // Close after 30 seconds → 3_000 to receiver, 7_000 back to sender
        env.ledger().with_mut(|l| l.timestamp = 30);
        client.close_stream(&id, &sender);

        assert_eq!(TokenClient::new(&env, &token).balance(&receiver), 3_000);
        // sender started with 1_000_000, deposited 10_000, gets 7_000 back
        assert_eq!(
            TokenClient::new(&env, &token).balance(&sender),
            1_000_000 - 10_000 + 7_000
        );

        let state = client.get_stream(&id);
        assert!(state.closed);
    }

    #[test]
    #[should_panic(expected = "already closed")]
    fn test_double_close_panics() {
        let (env, client, sender, receiver, token) = setup();
        let id = symbol_short!("s4");

        env.ledger().with_mut(|l| l.timestamp = 0);
        client.open_stream(&id, &sender, &receiver, &token, &1_000, &10);
        client.close_stream(&id, &sender);
        client.close_stream(&id, &sender); // should panic
    }

    #[test]
    #[should_panic(expected = "stream closed")]
    fn test_tick_on_closed_stream_panics() {
        let (env, client, sender, receiver, token) = setup();
        let id = symbol_short!("s5");

        env.ledger().with_mut(|l| l.timestamp = 0);
        client.open_stream(&id, &sender, &receiver, &token, &1_000, &10);
        client.close_stream(&id, &sender);
        client.tick(&id); // should panic
    }
}
