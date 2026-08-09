# Labyrinth Pinocchio

`labyrinth_pinocchio` records Magic Labyrinth's shared terrain randomness,
room resolution, portal state, and important match history on MagicBlock ER.
It does not own Ranked Matchmaking stake, custody, payouts, withdrawals, or
leaderboard state.

Configured program: [FbJpQqmiwrf9ZctR4sBQhKqgrUFqD8wfeD2VpZQukACH](https://explorer.solana.com/address/FbJpQqmiwrf9ZctR4sBQhKqgrUFqD8wfeD2VpZQukACH?cluster=devnet).

## v5 player-paid bootstrap

The selected final player creates Labyrinth accounts, pays their rent and
delegation fees, delegates Event Chunks first, and delegates Match last. The
permit allows either that player's existing Session Key or its corresponding
Main Wallet to pay and sign the bootstrap. Colyseus signs an immutable permit
and observes readiness; it does not submit or pay initialization/delegation
transactions.

See [Player-paid ER bootstrap](docs/PLAYER_PAID_ER_BOOTSTRAP.md) for the full
protocol, recovery path, cost estimate, and diagrams.

## Historical server-payer flow

Earlier v4 integrations used the backend signer as payer/submitter. That flow
is preserved as historical context only; new integration uses v5 player-paid
bootstrap. The v5 binary is deployed on Devnet at the configured program ID.
Existing v4 Match accounts remain runtime-readable; all new player-paid worlds
use v5.

## Checks

```sh
cargo test
cargo-build-sbf --manifest-path Cargo.toml --features bpf-entrypoint
```
