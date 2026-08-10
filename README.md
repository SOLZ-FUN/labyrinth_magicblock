# Labyrinth Pinocchio

`labyrinth_pinocchio` records Magic Labyrinth's shared terrain randomness,
room resolution, portal state, and important match history on MagicBlock ER.
It does not own Ranked Matchmaking stake, custody, payouts, withdrawals, or
leaderboard state.

Configured program: [FbJpQqmiwrf9ZctR4sBQhKqgrUFqD8wfeD2VpZQukACH](https://explorer.solana.com/address/FbJpQqmiwrf9ZctR4sBQhKqgrUFqD8wfeD2VpZQukACH?cluster=devnet).

## v6 compact player-paid bootstrap

New Labyrinth worlds use the compact v6 account. The selected player's existing
Session Key creates and delegates one 512-byte Match account. The Main Wallet
may fund that Session Key beforehand, but does not sign v6 bootstrap or
gameplay instructions. Colyseus signs immutable permits and observes state; it
does not submit or pay initialization/delegation transactions.

v6 replaces the v5 Match plus eight EventChunk bootstrap with a compact state,
Merkle-proven roster membership, room/spawn summaries, and a rolling event
hash. The final extracting player can submit `commit_match` or
`commit_and_undelegate_match` with its Session Key after a terminal result.

See [Player-paid ER bootstrap](docs/PLAYER_PAID_ER_BOOTSTRAP.md) for the full
protocol, recovery path, compact-account tradeoffs, and diagrams.

## Historical server-payer flow

Earlier v4 integrations used the backend signer as payer/submitter. That flow
is preserved as historical context only; new integration uses v5 player-paid
bootstrap. The v6 binary is deployed on Devnet at the configured program ID.
Existing v4/v5 Match accounts remain runtime-readable; new compact worlds use
v6.

## Checks

```sh
cargo test
cargo-build-sbf --manifest-path Cargo.toml --features bpf-entrypoint
```
