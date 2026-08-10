# Labyrinth Pinocchio

`labyrinth_pinocchio` records Magic Labyrinth's shared terrain randomness,
room resolution, portal state, and important match history on MagicBlock ER.
It does not own Ranked Matchmaking stake, custody, payouts, withdrawals, or
leaderboard state.

Configured program: [FbJpQqmiwrf9ZctR4sBQhKqgrUFqD8wfeD2VpZQukACH](https://explorer.solana.com/address/FbJpQqmiwrf9ZctR4sBQhKqgrUFqD8wfeD2VpZQukACH?cluster=devnet).

## Deployed proof

The current Devnet program is the v6 binary. It was deployed in slot
`482455884`, has ProgramData address
`4MHteiceWUfMXfsta66sKLsWDdvRF5p1XRs5WtmBUkb7`, and is upgrade-authorized by
`BrZ7WeDARopFAtiA3DP6c8F3eQFJnb4X8q1P5CaLQMjK`.

The current local build and the ELF fetched from Devnet have the same SHA-256:

```text
e707e80fc16e1883954d472849fd043b4ed7ee6de199ba319b719a2107d0c8aa
```

That is direct proof that the deployed program and this repository's v6 source
produce the same binary.

## Version history and why it moved

| Version | Account model | Player/Server authority | Why it changed |
| --- | --- | --- | --- |
| v4 | 5,088-byte Match plus configured EventChunks | Backend signer initialized, delegated, and committed | Initial ER/world prototype. It violated the desired player-pays boundary and made the server pay rent. |
| v5 | Same Match plus up to 16 EventChunks; default was 8 | Final player could initialize/delegate through a server-signed permit; Main-or-Session payer support | Removed Colyseus as transaction executor. It still required a large Match, chunk rent, and ten default bootstrap transactions. |
| v6 | One compact 512-byte Match; no EventChunks | Session Key signs bootstrap, gameplay requests, and terminal commit. Server signs permits/observes only. | Makes player-paid setup practical: compact roster proofs, deterministic room summaries, rolling event hash, two bootstrap transactions, and player-submitted commit/undelegate. |

v6 supports both origins through the same compact instructions:

- Ranked Labyrinth carries `match_kind = ranked` and validates the locked
  Ranked MatchRecord for the canonical Match ID.
- Labyrinth Direct carries `match_kind = direct` and requires a zero Ranked
  MatchRecord reference.

v6 intentionally trades v5's full event-payload archive for compact canonical
state plus a rolling event hash and terminal hash. Colyseus still owns detailed
movement/combat logs; the compact Labyrinth account anchors the verifiable
world/result facts without recording every frame-level event.

## v6 compact player-paid bootstrap

New Labyrinth worlds use the compact v6 account. The selected player's existing
Session Key creates and delegates one 512-byte Match account. The Main Wallet
may fund that Session Key beforehand, but does not sign v6 bootstrap or
gameplay instructions. Offchain Server signs immutable permits and observes state; it
does not submit or pay initialization/delegation transactions.

v6 replaces the v5 Match plus eight EventChunk bootstrap with a compact state,
Merkle-proven roster membership, room/spawn summaries, and a rolling event
hash. The final extracting player can submit `commit_match` or
`commit_and_undelegate_match` with its Session Key after a terminal result.

See [Player-paid ER bootstrap](docs/PLAYER_PAID_ER_BOOTSTRAP.md) for the full
protocol, recovery path, compact-account tradeoffs, and diagrams.

## Historical server-payer flow

Earlier v4 integrations used the backend signer as payer/submitter. That flow
is preserved as historical context only; new integration uses v6 player-paid
bootstrap. The v6 binary is deployed on Devnet at the configured program ID.
Existing v4/v5 Match accounts remain runtime-readable; new compact worlds use
v6.

## How to prove it from this repository

```sh
# 1. Source behavior: currently 31 tests pass.
cargo test --features bpf-entrypoint

# 2. Build the deployable v6 ELF.
cargo build-sbf --features bpf-entrypoint

# 3. Inspect the Devnet program ID, deployment slot, ProgramData, and authority.
solana program show FbJpQqmiwrf9ZctR4sBQhKqgrUFqD8wfeD2VpZQukACH --url https://api.devnet.solana.com

# 4. Fetch its deployed ELF and compare it with the local artifact.
tmp_dir=$(mktemp -d)
solana program dump FbJpQqmiwrf9ZctR4sBQhKqgrUFqD8wfeD2VpZQukACH "$tmp_dir/labyrinth-deployed.so" --url https://api.devnet.solana.com
shasum -a 256 target/deploy/labyrinth_pinocchio.so "$tmp_dir/labyrinth-deployed.so"

# 5. Inspect the source history that introduced each version.
git show --stat c0492a5  # v5 player-paid bootstrap
git show --stat fcc5bef  # v5 payer correction
git show --stat b257410  # compact v6 implementation
```

The expected result for step 4 is the matching hash shown above. The v6 test
suite includes compact layout/rent, 20-room bounds, Merkle membership mutation
rejection, rolling event-hash replay, terminal winner state, player Session-Key
commit eligibility, and v4/v5 compatibility checks.
