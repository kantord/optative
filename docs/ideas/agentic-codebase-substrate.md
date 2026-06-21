# Agentic Codebase Substrate

## Core idea

A type checker does three things: it **propagates** a change through everything that
depends on it, **locates** every site that is now wrong, and **fixes** (or refuses to
compile) the mechanical cases. This is enormously powerful — but it only works for
properties expressible as *types*.

A huge class of the properties we actually care about in a codebase are **not types**:

- every public function is documented *somewhere*
- every widget is exercised in *at least one realistic example*
- no file uses the deprecated API / `console.log` / the old error pattern
- every repo's CI conforms to the same *shape* (not the same bytes)
- the vendored copy still matches upstream
- the architecture doc still references symbols that exist

esto is **a type checker for the un-typeable**. You declare a set of items and a set of
**assertions** that must hold over them; esto continuously locates every violating site
and drives it back to compliance — mechanically when the fix is rote, or by dispatching
an **agent** when the fix needs judgment.

Run continuously over a project, this is less a tool than a **circulatory / nervous
system**: it senses (assertions over the live set) and actuates (reactions) to keep the
organism in its desired state. Documentation that goes stale gets re-flagged; a pattern
that drifts gets re-aligned; coverage that reopens gets re-filled. **Homeostasis for a
codebase's non-type properties.**

The payoff is the same as the type checker's, generalized: **changing a specification
propagates like changing a reused type.** "We now require X" stops being a migration you
remember to do everywhere and becomes a change the substrate ripples through the codebase
— except the propagation can be agentic.

## Why this is different from a linter

A linter *reports*. This *reconciles*. Three differences:

1. **Reactions, not just reports.** A violation is an item in a desired/current diff, and
   it carries a reaction — a command, or a prompt. The system closes the loop, it doesn't
   just print.
2. **Deletions and drift are first-class.** Remove the only doc for a function and the
   "documented somewhere" assertion *flips* — that is a reconcile event, not a thing a
   one-shot lint run happens to catch. The world drifting (a bug report, a dependency bump,
   someone else's commit) is a legitimate trigger.
3. **The fixer can be an agent.** When an assertion can be *checked* deterministically but
   *satisfied* only with judgment (write a real doc, add a meaningful example, adapt a CI
   file to a shape), the reaction is a prompt handed to an agent — not a codemod.

## Prior art & lineage

This is **integration novelty, not primitive novelty** — and the integration appears unbuilt.
Every ingredient ships, but the *union* — declarative non-type invariants + deterministic
detection + agentic remediation + a reconcile/convergence loop + coordinate-through-shared-state
— is not a named system as of 2026. (Strongest evidence: the negative result — the
"reconcile / desired-state" vocabulary is firmly attached to *infrastructure*, never to
LLM-remediated *source-code conventions*.) The defensible claim is the **synthesis and
vocabulary** ("a desired-state reconcile loop for code invariants"; "a type checker for non-type
properties"), never any single capability.

The lineage to build on (and cite), so we reuse 20 years of design instead of reinventing it:
- **MAPE-K / autonomic computing** (Kephart & Chess, *IEEE Computer*, 2003) — Monitor / Analyze /
  Plan / Execute over shared **Knowledge**. Structurally, this *is* a MAPE-K loop over source-code
  invariants with an LLM as the Plan/Execute actor.
- **Rainbow** (Garlan et al., 2004) — an externalized closed control loop with a reusable engine +
  per-domain customization (≈ "declare assertions, reuse the engine").
- **Architecture fitness functions** (Ford/Parsons/Kua) and **ArchUnit** — the direct ancestor of
  "declarative non-type invariants over code," with atomic/holistic and triggered/continual
  categories (≈ our one-shot/daemon modes). They *detect*; they don't reconcile — that's the half
  we add.
- **GitOps / Kubernetes operators, Puppet/Chef convergence** — the battle-tested
  reconcile-to-desired-state loop and coordinate-through-shared-state, but for infra with
  *mechanical* remediation. We borrow the loop; our remediation can be agentic.

**Nearest movers to differentiate against** (the gap is closing): GitHub *Continuous AI* /
Agentic Workflows — our edge is a *multi-assertion desired-state model + shared-state
coordination* vs their per-workflow scripts; Semgrep *Custom Workflows* — our edge is *general
non-type invariants + convergence* vs their security framing; Moderne *Moddy* / Codemod.com — our
edge is *standing invariants with a reconcile loop* vs migration campaigns. The moat is the
integrated desired-state model + general assertion language; the way to hold it is to **publish
the framing early.**

## The model: items × assertions × reactions × exemptions

Every rule is four things:

- **items** — how to enumerate the set (functions, files, widgets, repos, …). Real data,
  never hardcoded.
- **assert** — a *deterministic* check per item (or per item×assertion cell). Exit 0 = holds.
- **react** — what to do when it doesn't: `run:` a command (mechanical), or `prompt:` a
  template handed to an agent (judgment).
- **exempt** — a deliberate escape hatch: an allowlist file or an inline `esto-allow`
  comment, so a violation can be *consciously* accepted.

### The law: deterministic detection, pluggable reaction

Detection is **always** a static check (plus exemptions). The *what's-wrong* is therefore
always legible, reproducible, and reviewable — you can see the full violation set before
anything acts. Only the *fix* is pluggable, and only when it's a prompt is it
non-deterministic. This is what keeps an agentic codebase **auditable** instead of a black
box: the plan is deterministic; only the execution is fuzzy. (This is the COOKBOOK's
"deterministic planner / non-deterministic executor" promoted to a system-wide invariant.)

This split is the single most transferable lesson from the prior art. Facebook's Infer found
the *same* analysis at the *same* accuracy went from a **near-0% fix rate in batch mode to over
70% delivered at diff time** — *where and when* a finding surfaces matters more than its
accuracy. Meta's SapFix gates every machine patch behind compile + test checks *and* a mandatory
human reviewer before it is ever seen. So: detection deterministic and delivered in-workflow;
remediation gated, never trusted on its own report.

One hard caveat for the fuzzy half: **"the check passes" ≠ "the fix is correct."** Automated
program repair *overfits* 70–98% of the time on classic benchmarks — a patch that satisfies an
incomplete check by degenerate means (the archetype: deleting code until the tests go green). The
deterministic check is a *necessary gate, not a sufficient proof*: make assertions semantic and
hard to game, and verify the **post-condition** (re-run the detector, run the project's tests)
rather than trusting the agent's self-report.

## What the code looks like

The primitive layer stays exactly as esto is today — shell workers, opaque `key<TAB>value`,
string-equality change detection. The config layer is **not** a YAML rule list (too flat — no
composition, no reactivity, no modularity). It is **tauler with an assertion backend**: the same
reactive component runtime, where a node is an *invariant* instead of a UI panel, and "rendering"
means *reconciling*. You reuse tauler's frontend wholesale (`useJSONStream`, components, props,
`.map`, `events`); only the leaf node type (`<assert>` not `<panel>`) and the sink (reconcile, not
draw) differ.

Three syntaxes, each doing the job it's best at:
- **JSX** for *composition / structure* — which invariants exist, grouped into reusable
  components, mapped over live sets, conditionally included. (Trees; modularity.)
- **plain-JS pipes** for *linear dataflow within a node* — `source → augment → fingerprint`. Don't
  nest these as JSX (the inside-out anti-pattern); they live in the component body.
- **tagged templates** `sh\`…\`` / `jq\`…\`` / `prompt\`…\`` for the three embedded foreign
  languages (shell sources & mechanical fixes / JSON transforms / agent reactions). The tag
  escapes `${…}` for its context, which is what makes interpolation injection-safe.

```jsx
// repo.eso.jsx — the invariants this repo lives by, as a reactive program.
import Documented   from './invariants/Documented.jsx'
import Patched      from './invariants/Patched.jsx'
import { ConfigWidgetsCovered } from './invariants/coverage.jsx'

export default function plan() {              // "plan", not "render": it yields a desired state
  // ── linear dataflow: plain-JS pipes, typed items, no re-parsing ──
  const exports = sh`./esto/grep-exports.sh`  // source → items
    .jq`map({ file, name, sig: .signatureHash })`   // structured transform (items are JSON)
    .augment(addDocStatus)                          // enrich where jq can't reach

  const advisories = json`./esto/dep-advisories.sh` ?? []   // an external drift stream

  // ── composition: the TREE. modular, dynamic, conditional ──
  return <repo>
    {exports.map(fn => <Documented target={fn} />)}

    {/* desired state REACTS to a stream: this invariant only EXISTS while an
        advisory is open — it enters and exits as the stream changes. A flat
        list can't do this. */}
    {advisories.map(a => <Patched dep={a.name} fixedIn={a.fixedIn} />)}

    <ConfigWidgetsCovered />                  {/* a bundle of invariants, reused as a component */}
  </repo>
}
```

```jsx
// invariants/Documented.jsx — an invariant is a component (the modularity win)
export default function Documented({ target }) {
  return (
    <assert
      key={`${target.file}:${target.name}`}   // identity for the diff
      holds={target.hasDoc}                    // deterministic check (the gate)
      fingerprint={target.sig}                 // drives update / staleness
      fix={prompt`Add a concise JSDoc to ${target.name} in ${target.file}:
            what it does, its params, what it returns. Match the file's style.`}
    />
  )
}

// coverage.jsx — composition: one component templates a whole family of invariants
export const ConfigWidgetsCovered = () =>
  ['Radio','Switch','Slider','Toggle','ToggleGroup','NativeSelect']
    .map(w => <HasRealisticExample widget={w} />)
```

`fix={prompt\`…\`}` dispatches an agent; `fix={sh\`./esto/prepend-header.sh ${file}\`}` is a
codemod — same `fix` prop, the two reaction kinds. `<assert>` is `<text>` and `fix` is `on_click`:
data down (`props`), reactions up (`fix`/`events`), exactly tauler's contract.

### How it runs

A node is reconciled, not drawn. `<assert holds fingerprint fix>` becomes one item in its parent's
`OptativeSet`: `holds=false` → **enter** (dispatch `fix`); `fingerprint` moved since satisfied →
**update** (the proof may be stale → re-dispatch); item gone from the desired set → **exit**
(clean up the now-orphaned artifact). Convergence: resolving the violation stamps the per-item
state so the next pass sees it satisfied (COOKBOOK §"closing the loop"). The component author
writes the *invariant*; the runtime owns the diff, the state, and the dispatch.

### Raising the bar (spec-change propagation)

Because invariants are code, **editing one is editing the codebase's type system.** Tighten
`Documented` from "has a doc" to "has a doc *with an `@example`*", and on the next pass every
export that doesn't meet the *new* bar enters as a task — exactly as adding a field to a reused
type lights up every construction site. The spec change *is* the migration trigger.

### Raising the bar (spec-change propagation)

Because each rule is data, **editing a rule is editing the codebase's type system.** Tighten
`functions-documented` from "documented somewhere" to "documented *with a usage example*",
and on the next reconcile every export that doesn't meet the *new* bar lights up as a task —
exactly as adding a field to a reused type lights up every construction site. The spec change
*is* the migration trigger.

## Reactivity and the runtime

**The scripting system owns no loop — it's a reducer.** This dissolves the "where do the loops
live" question. The component program is a pure function `reduce(layout, inputs) → a supervisor
tree`, evaluated **when ticked**. It builds supervisor/target *definitions*; it does not run them on
a schedule. **Whoever embeds the scripting system owns the tick.** So:

- **esto** is a *raw embedding* — a generic host that ticks the reducer (once for `--once`, on a
  timer for `--rate-limit`, on an event for `--on`) and reconciles the resulting tree.
- **tauler** is a *domain-specific embedding* — its display loop ticks the same reducer; the tree's
  leaves are panels instead of asserts.

Same reducer, different embedders; the embedder decides cadence. (This is exactly
`tauler/docs/ideas/VISION.md`: "one event loop at the top, all layers below are pure reconcilers.")

Cadence is therefore a deployment mode, not an architecture (`management-api-vision.md`: *same API,
different mode*). One reducer program runs as:

- **one-shot** (`esto check`) — CI gate; converge once or exit non-zero. (What insane-forms does today.)
- **woken** — `esto serve --on git-push --on 'gh:issue.opened' --on inotify:src/`. The
  external drift sources of a *live* codebase — bug reports, dependency bumps, others'
  commits, upstream releases, CI results — are the events. A codebase doesn't drift on its
  own, but its *context* does; that is what makes it reconcilable.
- **daemon** — a continuous steward with `--rate-limit` for the agent pool.

The runtime is the `Supervisor` from `management-api-vision.md`: owns cadence + health + a
`ctl` control plane (`esto ctl violations`, `esto ctl reconcile <rule>`, `esto ctl logs
<rule>:<item>`). The same machine, queried the same way, whether it ran once or has been up
for a week.

## Interfaces & composition (a recursive supervision tree)

There are **two interfaces**, and conflating them is the trap:

- **At process boundaries** (engine ↔ `sh`/`jq`/worker/agent): **serialized lines, like shell** —
  `key<TAB>value`, value opaque or JSON. This is deliberate: it's what lets a worker be any
  language. The only additions over raw shell are that items are **keyed** (identity is what makes
  a diff possible) and the value may be JSON (parse once at ingest, not re-cut at every stage).
- **In-process** (combinator ↔ combinator): **lazy iterators of typed keyed items** —
  `Item = { key, value, …fields }`, `Source: () => Iterable<Item>`, `Transform: Iterable<Item> =>
  Iterable<Item>`. The mental model is shell's lazy streams, but the element is a typed item and it
  only re-becomes bytes at a boundary.

**Composition is a recursive supervision tree.** This is the heart of it, and it's why reconcile is
*not* a flat sink — it **recurses**. Every node has a dual identity:

- as an **item**, it is managed by its parent's set (it has enter/update/exit *relative to the parent*);
- as a **supervisor**, it owns a child set whose items have *their own* lifecycles.

So a node that is itself a lifecycle item may have children that are lifecycle items, arbitrarily
deep (this is tauler's `IncrNode`, whose `State` holds a child `OptativeSet`; and pipeline-vision's
"data depth = logic depth"). Example — "the folder exists" is an outcome, and *inside* that
satisfied outcome lives another layer, "a file exists in it":

```jsx
<folder path="x/">            {/* item in the repo's set; ALSO supervisor of its own child set */}
  <file name="a.txt" />       {/* item in the folder's set */}
  <file name="b.txt" />
</folder>
// folder.enter         → mkdir; init child set
// folder.reconcile_self→ folder still there? then reconcile the child set (the file assertions)
// folder.exit          → reconcile children to empty (remove files), then rmdir  (cascade teardown)
```

Three composition modes, each for a different relationship:

| mode | relationship | mechanism |
|---|---|---|
| horizontal pipeline | building one set | `Iterable<Item> → Iterable<Item>` (sh/jq/augment) — passes **data** |
| siblings (same kind, one parent) | peers, no order | one `OptativeSet`, reconciled together |
| **nesting (parent → child)** | **dependency / context / ordering** | child set lives *inside* the parent's satisfied state |

**Nesting is dependency, which is ordering** — this is how "B requires A" is expressed without a
scheduler: **B is a child of A.** A's child set doesn't exist until A has entered, so the folder is
guaranteed present before its files reconcile. Sequential dependency becomes **tree depth**, not a
DAG and not fragile through-the-world timing. (Genuine *peers* with no parent/child relation still
coordinate through the world — re-derive each pass — but a real dependency should be nesting.)

**The channels between layers:** *down* the tree flows **context** (props + the shared-world
handle) and the act of reconciling children; *up* flows an **`Outcome`** (ok / retry / fatal);
*sideways* between peer reconcilers flows **nothing but the shared world**. Crucially, you never
pipe one reconcile's **delta** into another's **input** as data — that's the lossy
forward-the-event-stream anti-pattern. Control recurses down; data does not flow sideways between
reconciles.

**"Supervisor" is a role, not necessarily a thread.** A supervisor is anything that owns a set and
knows it must run the reconcile algorithm on it. Two implementations, chosen per layer:

- **Default — in-memory, synchronous:** the supervisor is just a node; the whole tree is reconciled
  in one depth-first walk per pass (tauler's per-frame render reconcile). Most assertion layers are
  this — "file exists" is cheap and synchronous, riding the parent's cadence.
- **Promote to its own thread/cadence only when a layer needs *independent* liveness or rate:**
  tauler does this for exactly one layer — the data loop runs on its own 50 ms thread because data
  sources crash and emit on their own schedule. The codebase analog is a layer of long-running
  agent jobs you poll. So **cadence is per-layer and mostly inherited from the parent; a thread is
  the exception** you reach for when a layer's lifecycle must run independently of its parent's
  (`health-vision`'s Supervisor; `datapipe-vision`'s "each stage at a different rate").

## Relationship to optative / tauler / esto today

- **optative (the library)** is the substrate: `Lifecycle` + `OptativeSet`.
- **tauler** is a frontend over it: desired state = a *pure function of inputs* (JSX over
  streams → UI tree), reconciled into live windows.
- **esto** is the shell frontend: desired state = scripts → an item set, reconciled into
  side effects.

This doc proposes a **second tauler backend over the same runtime**: the *same* reactive JSX
component frontend, but its nodes are *invariants* and its sink is *reconcile-the-codebase* instead
of *draw-pixels*. tauler renders streams → a UI tree → windows; this renders streams (repo + external
events) → an invariant tree → conformance. **Same architecture, same syntax, different leaf node and
sink.** It is emphatically *not* a new spec language — it reuses `useJSONStream`, components, props,
`.map`, and the `events` wiring wholesale; `<assert>`/`fix` replace `<panel>`/`on_click`.

The unification: one `Supervisor` runtime under three backends — typed/live stages (tauler's `Child`
handles, in Rust), externalizable stages (esto shell workers), and codebase invariants (the JSX
assertion backend). "Make esto work like tauler" means *give esto that runtime and that reactive
frontend* — so yes, adopt the JSX (the earlier "don't adopt JSX" was wrong); only the backend leaf
type and sink are domain-specific.

## Multi-agent without a scheduler

The interesting consequence: a "team" of agents maintaining a codebase needs no message bus
and no orchestrator. Each invariant is a reconcile node; each fans out agents on its own
violations; **agents coordinate through the shared world (the repo), not by talking to each
other.** This is the Kubernetes model applied to agents: many controllers, one shared state,
emergent coordination. Ordering, where it's truly needed, is **nesting** — a dependent invariant
is a *child* of its prerequisite, so it can't reconcile until the parent is satisfied (see
*Interfaces & composition*); genuine peers coordinate through the world.

A conversational agent loop is then the **degenerate single-item case**: desired = "request
satisfied", current = "conversation so far", each turn a reconcile step. The substrate is the
generalization — from one conversation converging to N items converging, coordinated through
the world.

## The keystone to build

Not a new primitive — a frontend + a runtime: the **reactive JSX assertion backend** (invariants as
components; `sh`/`jq`/`prompt` tags; a recursive supervision tree of `<assert>` nodes) running over
the `Supervisor` runtime with event wakeup. That turns esto from "a diff tool you invoke" into "the
nervous system a living codebase runs on." A consumer project's existing one-off checks (coverage,
docs, drift) stop being separate scripts and become *invariant components in one reactive program*,
continuously enforced and agentically self-healing.

## Convergence safety (control-loop hygiene)

Solved problems in the reconciler lineage — copy the patterns, don't rediscover them:

- **Level-triggered, not edge-triggered.** Recompute desired-vs-current fresh every pass; treat
  webhooks/events as *hints about when to look*, never as the truth about what changed. A missed
  event then never means a permanently missed fix.
- **Idempotent by construction.** A remediation that's destructive when run twice is a ticking
  time bomb. Verify the post-condition (the invariant now holds); don't trust the actor.
- **Rate-limit + backoff + global token bucket.** Copy controller-runtime's defaults: per-item
  exponential backoff (5 ms → 1000 s) plus a global bucket (~10 QPS, burst 100). Prevents reconcile
  storms and thundering herds — and, for agents, runaway cost.
- **Converge-or-escalate.** Bound retries per scope (Erlang/OTP `intensity`/`period`); when a
  remediation keeps failing, **stop and surface to a human** — don't loop. Budgets must not
  *multiply* across the repo→file→rule hierarchy (10×10 = 100 retries).
- **Hysteresis / deadband, asymmetric.** Be aggressive about *adding* a missing invariant,
  conservative about *removing/undoing* (HPA: scale-up immediate, scale-down 5-min max-over-window).
  This kills the fix→revert→fix oscillation.
- **Don't fight a legitimate owner.** The ArgoCD-vs-HPA perpetual sync loop is our exact hazard
  when we "remediate" something another owner (a human, another bot) controls. Use field-scoped
  ownership / ignore rules so two actors coexist on one file.

## Adoption & trust (the binding constraint)

Prior art is unanimous: **trust is the scarce resource, and delivery beats accuracy.**

- **Deliver in-workflow, at diff time, to the diff author** — the highest-ROI decision available
  (Infer's 0%→70%). A batch dashboard gets ignored.
- **≤10% *effective* false positives** for anything shown to a developer, where *the developer*
  defines "didn't want to see it." Cross it and trust collapses for the *whole system*, not one
  rule (Google's FindBugs failure → Tricorder success). Track **action/fix rate**, not a theoretical
  FP rate, with a per-rule "not useful" signal feeding tuning.
- **Do the work for teams; never issue unfunded mandates.** File the fix, don't file a demand.
- **Make machine diffs look human** and small; **author ≠ reviewer** (the actor that proposes a
  change is not the authoritative gate); **risk-tier autonomy by blast radius** (suggest-only for
  high-impact; auto-land only narrow, near-zero-FP classes); keep a **global kill switch**.
- **Infrastructure-level guardrails, not prompt-level**, for agents: sandboxes, branch isolation,
  deny-lists, least-privilege short-lived creds, idempotency keys. *"If an agent can be told to
  bypass a guardrail, the guardrail doesn't exist."*
- **Calibrate against complacency.** Reviewers *will* rubber-stamp mostly-correct agent PRs (the
  vigilance limit is real) — keep diffs legible, sample-audit, force genuine engagement.
- **Roll out gradually:** dryrun → warn → enforce; canary a few repos; group PRs (target <5/repo/week).

## Exemption governance (anti-rot)

Exemptions keep the system humane, but **every team that scaled this ended up with a suppression
file that only grows.** Design against rot from day one:

- A **monotonic, self-decrementing ratchet** — per-rule/per-file counts, auto-decrement on fix,
  explicit approval required to *increase* (the Notion `eslint-seatbelt` pattern).
- Every exemption carries **owner + justification + expiry**; never silent. Prefer fix >
  exempt-with-sign-off (the OPA exemption hierarchy).
- **Semantic anchoring (symbol/AST), not line numbers**, so reformatting doesn't churn the baseline
  and silently launder new violations.
- Track **exemption debt as a first-class metric that must trend down** — quite possibly its own
  rule in the registry ("every exemption has a reason and an unexpired date").

## Open questions / tensions

(Several earlier opens are now answered by the prior art — see *Convergence safety*, *Adoption &
trust*, *Exemption governance*. What genuinely remains:)

- **Ordering vs no-scheduler** — *resolved by nesting.* A dependency is expressed as parent→child:
  the child set lives inside the parent's satisfied state, so it can't reconcile until the parent
  has entered (see *Interfaces & composition*). Sequential dependency = tree depth, not a scheduler.
  Peers with no parent/child relation fall back to coordinate-through-the-world. Residual edge: a
  dependency that is neither a clean nesting nor a peer (a cross-cutting "A and B before C" join)
  may still want an explicit higher node that owns all three.
- **Unstable keys.** A rename reads as exit+enter (history lost). Doc/example continuity across
  renames likely needs content- or symbol-stable keys, not paths.
- **Value double-duty.** The fingerprint is both change-detector and worker payload. "Assert on X,
  hand the agent Y" needs richer payloads (JSON the worker parses).
- **Outcome model.** Agent reactions need `Retry`/`Fatal`, not exit 0/1 — the
  `lifecycle-status-vision.md` `Outcome<E>`, designed and unbuilt, and load-bearing here
  (rate-limited LLM pool, partial progress).
