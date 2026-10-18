# PROTOTYPE — spec-enforced checks

Throwaway. Answers one question: **can a Gherkin-like spec be the desired state of an
esto reconciler whose items are the checks (unit / type / integration) that enforce it?**

- `spec/auth.spec` — the spec. `Rule <id> <kind>:` headers, free-text bodies.
- `lib/spec.mjs` — the parser (pure; the part worth keeping).
- `spec.op.tsx` — the reconciler. Desired = rules keyed `<feature>/<id>`, valued by a hash
  of the body. Observed = `// @spec <feature>/<id> #<hash>` markers grepped from the
  kind's test dir (`tests/unit`, `tests/types`, `tests/e2e`).
- `src/` — a tiny Vite app (token verification + login lockout) to have something to check.

Seeded so every lifecycle event fires once:

| rule | state | event |
|---|---|---|
| `token-expiry/rejects-expired` | check marker matches | unchanged |
| `token-expiry/expiry-required` | type check matches | unchanged |
| `token-expiry/clock-skew` | no check | enter |
| `login/lockout` | marker hash is from an older rule text | update |
| `token-expiry/accepts-unsigned` | check exists, rule removed | exit |

## Run

```sh
pnpm install
pnpm spec          # diff: what's missing / stale / orphaned
pnpm spec:tasks    # write one agent task per delta to tasks/
pnpm test          # vitest: unit + type checks
pnpm e2e           # playwright against the Vite app
```

Then edit `spec/auth.spec` and run `pnpm spec` again.

## Findings so far

- Whitespace-only edits (re-indent, aligning steps or tables) don't make checks stale:
  the body is normalized before hashing.
- **Gap in esto:** changing a rule's kind (`<type>` → `<unit>`) enters a new unit check but
  never exits the old type check. esto discovers units only from leaves in the tree
  (`crates/optative-script/src/engine.rs`, the `by_kind` loop), so a unit with zero
  desired items never observes, and its orphans are never reported.
