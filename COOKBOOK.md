# esto cookbook

`esto` is the CLI front-end to [optative](./README.md): a **generic keyed-set reconciler
with pluggable, shell-defined reactions.** Where the optative library asks you to implement
a Rust `Lifecycle` trait, `esto` lets you express the whole thing as shell commands — so any
"make reality match a desired state" job becomes three little scripts.

This cookbook is about what you can *do* with that. The surprising part is how much range a
deliberately tiny interface has once you start abusing it.

## The interface, in one breath

```
esto --once \
  --from '<cmd emitting current state>' \   # key<TAB>value per line
  --to   '<cmd emitting desired state>' \    # key<TAB>value per line
  --enter  './on-new.sh'   \   # key<TAB>value           (key only in --to)
  --update './on-change.sh' \  # key<TAB>old<TAB>new      (key in both, value differs)
  --exit   './on-gone.sh'      # key<TAB>value           (key only in --from)
```

Workers are long-lived processes: esto writes one task per line on their stdin; they reply
`done<TAB>key` (or `error<TAB>key<TAB>msg`). Drop `--once` and add `--rate-limit` /
`--reingest-every` to turn the one-shot check into a forever-running controller.

The value is **opaque** — esto never parses it. Change detection is plain string equality.
That single decision is what makes everything below possible.

## When does this pattern fit? (the litmus test)

Three ingredients. If all hold, esto fits:

1. **Enumerable** — you can list "what is" and "what should be" as `key → value` lines.
2. **Fingerprintable** — the `value` encodes *"has this changed"* (a hash, version, signature,
   or even a yes/no predicate).
3. **Reactable** — handling a delta item is either **mechanical** (a command) or expressible as
   a **per-item prompt** (hand it to an AI agent when the fix needs judgment).

Two orthogonal choices then pick the flavor:

- **Reaction**: mechanical worker vs prompt → agent fan-out.
- **Cadence**: `--once` (CI / manual check) vs loop (live controller).

---

## Idioms

The interface is three knobs — *how each side is enumerated*, *what "equal" means via the
value*, and *what happens on enter/exit/update*. Here's how far that goes.

### 1. Invariant-as-constant-target — enforce a predicate over a set

The value doesn't have to be content. Make `--to` a **constant** and let `--from` *measure
reality*. The delta is exactly the violators. Example: "every Rust file must start with an
SPDX license line."

> **When NOT to use esto for this.** A constant target means the diff degenerates to "filter the
> violators" — the enter/exit/update lifecycle goes unused. If the reaction is simple (fail CI, or
> one mechanical fix), a plain build-time check (enumerate → filter → exit non-zero) is simpler and
> has fewer moving parts; don't reach for esto. The idiom earns esto only when the per-item reaction
> is heavy — e.g. an AI agent per violator across many items, where you want the task-file fan-out,
> loop-until-dry, and worker pooling. Full esto is for when *both* sides vary.

```bash
# from: measure — does each file satisfy the predicate?
cat > from.sh <<'SH'
#!/usr/bin/env bash
for f in $(git ls-files '*.rs'); do
  head -1 "$f" | grep -q 'SPDX-License-Identifier' && v=ok || v=missing
  printf '%s\t%s\n' "$f" "$v"
done
SH

# to: the invariant — every file should be "ok"
cat > to.sh <<'SH'
#!/usr/bin/env bash
for f in $(git ls-files '*.rs'); do printf '%s\tok\n' "$f"; done
SH

# worker: enforce it (mechanical — no AI needed)
cat > add-header.sh <<'SH'
#!/usr/bin/env bash
while IFS=$'\t' read -r file old new; do
  [ -z "$file" ] && continue
  { echo '// SPDX-License-Identifier: MIT'; cat "$file"; } > "$file.t" && mv "$file.t" "$file"
  printf 'done\t%s\n' "$file"
done
SH
chmod +x from.sh to.sh add-header.sh

esto --once --from ./from.sh --to ./to.sh --update ./add-header.sh
```

Same keys on both sides, so only `--update` fires — and only for `missing → ok`. Once a file
is fixed, `from` reports `ok`, matches `to`, and never re-triggers. This one idiom covers a
huge class: "every file imports the logger," "no `console.log`," "every endpoint has auth,"
"every package declares a license," …

### 2. `--to = transform(--from)` — "desired = current after the rule"

Desired state is often just *current state run through a tool*. Vendored dependency drift:
`--from` hashes the committed copy, `--to` hashes a fresh re-fetch of the same thing; the
worker re-syncs whatever diverged. Self-canonicalization: `--to = format(--from)` finds every
item not already in normal form.

### 3. Empty-side — turn reconcile into pure create or pure delete

Make one side emit nothing (`true` is a valid command that outputs nothing):

```bash
esto --once --from 'true'        --to ./list-desired.sh  --enter ./create.sh   # provision all
esto --once --from ./list-now.sh --to 'true'             --exit  ./delete.sh   # tear down all
```

One tool, two opposite bulk operations. And note: **deletions are first-class.** Most ad-hoc
migration scripts forget removals; here `--exit` gives you GC for free.

### 4. Loop-until-dry — free resumability & convergence

Because `--once` is a stateless diff, re-running only resurfaces *still-divergent* items. So a
flaky or partial run self-heals: just run again. Wrap it to converge to zero:

```bash
while :; do
  rm -rf tasks; mkdir tasks
  esto --once --from ./from.sh --to ./to.sh --update ./emit-task.sh
  [ -z "$(ls -A tasks)" ] && break        # no work left → converged
  ./handle-all-tasks.sh tasks             # do the work; next pass re-checks
done
```

### 5. Dry-run via identity worker — plan/apply for free

Swap the real worker for one that only logs and acks → esto becomes a pure **planner**.

```bash
cat > plan.sh <<'SH'
#!/usr/bin/env bash
while IFS= read -r line; do
  echo "WOULD CHANGE: $line" >&2
  printf 'done\t%s\n' "$(printf '%s' "$line" | cut -f1)"
done
SH
esto --once --from ./from.sh --to ./to.sh --update ./plan.sh   # plan
esto --once --from ./from.sh --to ./to.sh --update ./apply.sh  # apply
```

### 6. Compound keys — cartesian fan-out

The key is just a string; make it composite to fan out across dimensions. i18n is the cleanest
case — one task per (locale × source-string) cell:

```bash
# to: every (locale, string) should match the CURRENT source-string hash
cat > to.sh <<'SH'
#!/usr/bin/env bash
for loc in en es fr de; do
  while IFS=$'\t' read -r id src; do
    printf '%s:%s\t%s\n' "$loc" "$id" "$(printf '%s' "$src" | sha1sum | cut -c1-12)"
  done < source-strings.tsv
done
SH
# from: locale:id -> hash of the source as it was when last translated
# → enter = new string to translate, update = source changed → re-translate,
#   exit  = orphaned translation to delete.
```

`key=component:viewport` gives you a visual-test matrix; `key=repo:setting` gives org-wide
config governance. Any product of dimensions becomes one task per cell.

### 7. Value = hash of a *subset* — dial the sensitivity

You decide what "changed" means by choosing what you fold into the value. Hash only a function's
*signature* to ignore body edits; normalize (format/strip comments) before hashing to ignore
cosmetic churn; include a `version` field to react to bumps only. The value is both the
change-detector and the worker payload — keep that in mind when picking it.

### 8. Prefix-filter — pilot, then roll out

Filter both sides by key prefix to process a subset first, eyeball the result, then widen:

```bash
esto --once --from './from.sh | grep ^src/components/' \
            --to   './to.sh   | grep ^src/components/' --update ./apply.sh
# happy? drop the grep and run the whole tree.
```

### 9. Two-list join — reconciliation as `comm`/`join` with reactions

Keys are identities, so esto is a set-join that *acts* on the diff: users in system A vs system
B → `enter` provisions in B, `exit` deprovisions. Accounts, entitlements, feature flags, DNS.

---

## The AI fan-out pattern

When the per-item fix needs judgment, the worker shouldn't *do* the work — it should **emit a
prompt** and ack. esto becomes a deterministic planner that produces a task set; an LLM agent
then executes it.

```bash
cat > emit-task.sh <<'SH'
#!/usr/bin/env bash
mkdir -p tasks
while IFS=$'\t' read -r key old new; do
  [ -z "$key" ] && continue
  cat > "tasks/$key.md" <<EOF
# Task: $key
old=$old new=$new
<instructions for an agent to reconcile this one item>
EOF
  printf 'done\t%s\n' "$key"
done
SH
```

Why emit prompts instead of calling a headless LLM per item? One *already-running* orchestrator
that reads `tasks/` and fans out sub-agents shares context, runs them in parallel, and is far
cheaper than N cold per-item invocations — and you get a human checkpoint between "here's the
plan" and "go." The deterministic diff stays reproducible; only the execution is non-deterministic.

---

## Chaining reconcilers

A uniform TSV/shell interface composes like pipes. Two forms:

- **Pipeline** — an upstream reconcile changes the world; a downstream one re-reconciles only what
  moved (`docs → search index`, `schema → codegen → callers`). Don't recompute the world each link.
- **Hierarchical** — a worker *is* an esto: the parent reconciles the top level, and each item's
  worker invokes `esto --once` for that item's sub-state (`org → repos → settings`,
  `project → files → symbols`).

Both work today with shell glue — a worker writes the next esto's input, or simply runs `esto`.

**The principle that keeps chains sound: compose through the world (shared state), not by forwarding
the event stream.** It's tempting to pipe one esto's enter/exit/update events into the next, but a
reconcile emits a *delta* (three event kinds) while the next stage wants a *set* — reconstructing a
set from a delta is lossy and couples the stages to each other's history. Instead, let each stage
re-derive `--from`/`--to` from reality. Then every link stays a stateless function of
`(current, desired)`: correct when run standalone, debuggable in isolation, and still convergent under
loop-until-dry. Layer the "only do what changed upstream" optimization on top (scope the downstream
`--to` to the upstream's changed keys) *without* making correctness depend on it.

Keep esto the sharp single-set primitive — orchestrate chains *above* it (a script, `make`, or a
meta-reconcile whose items are "reconcilers to run"). Don't grow a DAG scheduler inside esto.

**Hazard — feedback cycles.** Chained reconcilers can loop (A mutates the world → B mutates → re-triggers
A). Convergence requires the chain to be a DAG (or monotone toward a fixpoint) and every worker to be
idempotent — the same property loop-until-dry already assumes, now across links.

## Enumerators — let AST/LSP tools produce `--from`/`--to`

For code-aware reconciles, the `--from`/`--to` scripts often need real program structure, not regex.
Reach for an analysis tool as the **enumerator/fingerprinter** — esto still owns the reaction:

- **AST (e.g. ts-morph, or the bare TypeScript compiler API)** — in-process, single-language. Great for
  "list the symbols this file shows and classify each by its import source" (→ a known dependency, a
  local definition, an external lib). The classification *is* the value, which lets the worker act
  precisely. Prefer the compiler API you already depend on over adding a library, when the analysis is
  just syntactic (imports + identifiers).
- **LSP client (e.g. multilspy)** — drives a real language server for IDE-grade def/refs/hover across
  many languages. Heavier (a server process per language); worth it for multi-language repos or feeding
  live semantic context to an LLM, overkill for a single-language in-repo check.
- **Structural pattern matchers (ast-grep, semgrep)** — tree-sitter/AST matching with metavariables, fast,
  JSON out. The right enumerator when the value is a *structural predicate* ("files/nodes matching pattern
  P") — migrations and lint-style invariants (idiom #1). Key limit: they **match but don't resolve** — no
  symbol binding, scope, or types. So for "classify this symbol by where it's defined/imported" you still
  need an AST/LSP tool plus your own join; a matcher would just add a dependency and re-do the walk. ast-grep
  is light (single binary); semgrep adds dataflow/security rules but is heavier and weaker on TSX.

The division of labor: the analysis tool answers *"what exists and what is it"* (ingredients 1–2);
esto answers *"what's missing/changed and what to do"* (ingredient 3).

## Example catalog

**Vendoring / derived-artifact drift** — current = committed copy, desired = regenerated.
- Vendored UI components, copied utilities, forks: `item → re-vendor hash`; update = re-sync + adapt.
- Codegen (OpenAPI / GraphQL / protobuf / DB schema): `symbol → spec hash`; mechanical regen + agent adapts callers.
- Design tokens → CSS variables: `token → value hash`.

**Coverage gaps** — current = what exists, desired = what *should* exist. The agent's sweet spot.
- **i18n**: `locale:msg-id → source hash` (idiom #6).
- **Docs drift**: `public export → signature hash`; enter = write docs, update = update docs.
- **Test drift**: `function → body hash`; enter = write a test, update = review the test.
- **Story / example coverage**: `component → props hash`; enter = scaffold a story.

**Large-scale code migration** — current = old pattern, desired = new.
- Codemod-with-adaptation: `file → "uses old API?" predicate` → agent migrates each file.
- Lint-rule rollout: `violating file → violation set` → fix one task at a time.
- **Maintained port** (the standout): keep a re-implementation tracking its upstream —
  `--to` = upstream component hashes, `--from` = your port; every upstream change becomes a port
  task. The reconcile loop is what stops a fork from rotting: normally ports die because nobody
  tracks upstream diffs. Caveat: a semantic port is adapt-not-copy, so it's agent work with
  per-item verification.

**Incremental pipelines**
- RAG / search index: `doc → content hash` → embed / re-embed / drop. Mechanical, continuous.
- Asset derivatives: `image → hash` → regenerate thumbnails, generate alt-text (agent).

**Self-maintenance (meta)**
- **Memory ↔ reality**: `note → referenced-symbol existence/hash`; update/exit when a note cites
  a file or flag that changed or vanished → agent re-verifies and rewrites it. Directly attacks
  the "notes go stale" problem.
- Skills / docs ↔ the code they describe.

**Continuous controllers** (esto's native habitat — drop `--once`)
- IaC / k8s-style resource reconcile; repo-settings & CI-template governance across many repos;
  issue/PR triage; process & connection pools (optative's original use cases).

---

## Caveats

- **Unstable keys**: a rename reads as `exit` + `enter`, not `update` — history is lost. Watch
  out for file moves and renamed identifiers.
- **Ordering / dependencies**: this is a *set* reconcile with parallel fan-out. Work that must
  run in sequence (ordered DB migrations) doesn't fit cleanly.
- **Expensive desired-state**: if computing `--to` is costly (a network re-fetch), prefer
  `--once` on a schedule over a tight loop.
- **Idempotency**: non-idempotent reactions are dangerous in the continuous loop. Make workers
  safe to re-run (the loop-until-dry idiom assumes this).
- **Value double-duty**: the value is both change-detector and worker payload. Fine until you
  need "compare on X but hand the worker Y" — then carry both in the value (e.g. JSON the worker
  parses) and fingerprint the part you care about.

---

## See also

- [README.md](./README.md) — the optative Rust library (`Lifecycle` trait, `OptativeSet`).
- `esto --help` — the protocol reference.
