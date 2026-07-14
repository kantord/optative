# Authoring Model: Targets, Sets, Decorators, Supervision

How a user *extends* the assertion backend (see `agentic-codebase-substrate.md`): the surface for
defining custom reconcilers, custom lifecycle targets, and — the hard part — **sets defined by
pattern rather than one item at a time.** The runtime is tauler's reactive component model
(`useJSONStream`, components, props, `.map`, `events`); this doc is about the *vocabulary a user
writes in it*.

## Core idea

There are exactly four things, and only three are authored:

- **Supervision** — *automatic*, never authored. A node that renders children owns a child set and
  is therefore a supervisor. (Decision below.)
- **Target** — *what one reconciled item is* (its identity + check + lifecycle hooks). The leaf
  primitive. Defined with `defineTarget`; parameterized by props.
- **Decorator** — *the environment a subtree reconciles in* (rate limit, agent pool, retry budget,
  cadence, dry-run, exemption scope). A wrapper component that injects Context. Does **not** change
  what the items are.
- **Set** — *which items exist*. A node's children flatten into its set; **expander** components
  (`Each`/`Match`) turn a source + a template into many items, so you define a *pattern*, not a list.

The throughline: **structure is inferred, behavior is injected, the leaf primitive is small.** Users
mostly write components and `.map`s; they reach for `defineTarget` only to teach the system a new
*kind* of object, and for a decorator only to wrap a subtree in a shared resource.

## Design stance: a minimal core that integrates, not a plugin host

The system is a **small set of generic primitives centered on integrating *other* systems** —
shell, jq, ast-grep/semgrep, LLMs — not a large framework with a plugin API. The built-ins are only:

- the **reducer** — `(layout × inputs) → a supervisor tree`, evaluated when the embedder ticks (see
  `agentic-codebase-substrate.md` §runtime: the scripting system owns no loop);
- the **item / stream model** — keyed items; lazy typed iterators in-process; `key<TAB>value` at boundaries;
- the **reconcile combinator** — diff a set → enter/update/exit;
- `defineTarget` — a `Lifecycle` as a plain object.

Everything else — `<assert>`, `<Each>`, `<Match>`, `<Documented>`, `<RateLimited>` — is **userland
composition: partial application and code-dedup, not external plugins.** `<assert>` is
`extendTarget(…)`; `<Documented>` is `<assert>` with props bound; `<Match>` is `<Each>` with a file
source. There is **no plugin boundary to cross** — a "component" is just a function that returns nodes.

Likewise, **sources are integrations, not features.** There is no universal `glob` primitive that
pretends to enumerate everything; you reach for the right external tool and wrap it: `sh\`git ls-files\``
for files, `astgrep\`…\`` / `semgrep\`…\`` for structural code sets, a bespoke script for anything else.
The system's job is to *glue and reconcile* what those tools emit — never to reimplement them. (This is
tauler's `structured-unix-philosophy.md` applied to reconciliation.)

## Supervision is automatic (decision)

> **A node becomes a supervisor iff it renders children.** No flag, no opt-in. Children ⇒ the node
> owns a child `OptativeSet` ⇒ it is reconciled as a supervisor. No children ⇒ leaf.

This is emergent from tree position, exactly as tauler's `IncrNode` becomes a supervisor precisely
when its `State` holds a child set. The runtime does the depth-first walk; "supervisor" is never a
thing the author declares.

The **one** thing that is *not* inferred is **cadence** — whether a layer needs its own thread /
independent liveness. Structure can't infer that, so it's an explicit opt-in (a decorator, e.g.
`<OwnThread interval="5s">`). Default: inherit the parent's pass. Opt in only when a layer's
lifecycle must run independently of its parent's (the tauler data-loop case).

### A node is a self-assertion ⊕ a child-scope (don't split them)

There is no separate "scope node" vs "assertion node" — and trying to split them is a mistake,
because the same thing is often *both* (a folder both *asserts it exists* and *scopes its files*).
Every node has two **orthogonal, optional** aspects:

- a **self-assertion** — `holds` + lifecycle (what must be true *about this node itself*);
- a **child-scope** — its children (a sub-set it supervises).

All four combinations are valid: assertion-only (`<assert>`), scope-only (a pure group like
`<repo>`/`<group>`, no self-check), **both** (the folder case), or neither (a no-op). The clarity
comes not from splitting node *types* but from how nesting **reads**: *children are asserted "given
the parent holds."* A child set lives inside its parent's satisfied state, so a child's context is
exactly "its nearest asserting ancestor is satisfied." Read every nesting as a conditional — *given
`x/` exists, these files exist in it* — and the dual nature stops being confusing.

## Targets — `defineTarget`, parameterized by props

A target answers: identity, fingerprint, the deterministic check, and the lifecycle reactions —
**all as functions of the node's props.** Props are how an instance is configured.

```js
const File = defineTarget({
  key:    (p) => p.path,                          // identity for the diff
  value:  (p) => p.hash ?? hash(read(p.path)),    // fingerprint — from a prop OR computed
  holds:  (p) => exists(p.path),                  // deterministic check (the gate)
  enter:  (p) => sh`mkdir -p ${dir(p.path)} && touch ${p.path}`,
  update: (p) => /* re-sync to desired */,
  exit:   (p) => sh`rm -f ${p.path}`,
})
// usage — props parameterize the item:
<File path="x/a.txt" hash={expectedHash} />
```

The built-ins are not privileged — `<assert>` is just a seeded `defineTarget`:

```js
const Assert = defineTarget({
  key:    (p) => p.key,
  value:  (p) => p.fingerprint,
  holds:  (p) => p.holds,            // an already-evaluated boolean prop
  enter:  (p) => p.fix(p),           // violation → run the fix
  update: (p) => p.fix(p),           // fingerprint moved → re-run (staleness)
  exit:   (p) => p.onExit?.(p),
})
// <assert key holds fingerprint fix/>  is  <Assert …/>.
```

This is the optative `Lifecycle` trait (`key`/`State`/`enter`/`reconcile_self`/`exit`) surfaced as a
JS object. esto's `HookItem`, tauler's `PanelSpec`, and a user's `File` are three instances of the
same trait; nothing about the built-ins is special.

## Composing & overriding targets ("subclass", really compose/override)

A new target is usually a *variation* of an existing one — override some hooks, keep the rest.
`extendTarget(base, overrides)` shallow-merges hooks:

```js
// REPLACE hooks: a documented-export assertion is an Assert with a different check + fix
const Documented = extendTarget(Assert, {
  key:   (p) => `${p.target.file}:${p.target.name}`,
  value: (p) => p.target.sig,
  holds: (p) => p.target.hasDoc,
  fix:   (p) => prompt`Document ${p.target.name} in ${p.target.file}: what/params/returns.`,
})
```

A hook may also **wrap** the base (call "super") by taking it as a second argument — that's a
decorator *on a single hook*:

```js
const Audited = extendTarget(File, {
  enter: (p, base) => { log("creating", p.path); return base(p) },   // wrap, then delegate
})
```

So "subclass" = `extendTarget`; **replace** by ignoring `base`, **compose/wrap** by calling it.
No class hierarchy — just hook objects merged and optionally chained.

## Decorators — wrap a subtree, inject Context (the HOC pattern)

Confirmed: the HOC here is the **decorator pattern**. It wraps a subtree and changes the
*environment* the descendants reconcile in, **without touching what the items are.** It passes the
reconcile logic through to children, adding its own wrapper — typically by providing Context (the
shared resources tauler already threads through `Context`):

```jsx
<RateLimited pool="agents" qps={10}>     {/* provides an agent-pool ctx; fixes acquire from it */}
  <RetryBudget intensity={5} period="30s">  {/* converge-or-escalate budget for the subtree */}
    <Documented target={fn} />
  </RetryBudget>
</RateLimited>
```

`RateLimited` doesn't change `Documented`; it changes how `Documented`'s `fix` is dispatched — a
descendant `fix` does `ctx.agentPool.acquire()` and, if saturated, yields `Outcome::Retry`
(literally `lifecycle-status-vision.md`'s example). Decorators are the natural home for every
cross-cutting concern: `<RateLimited>`, `<RetryBudget>`, `<DryRun>` (run as planner, dispatch
nothing), `<ExemptScope file="…">`, `<OwnThread interval="5s">` (the cadence opt-in).

The two composition layers, kept distinct:
- **`extendTarget`** changes *what one item is* (its hooks).
- **decorator** changes *the environment a subtree reconciles in* (its Context).

## Defining sets — by pattern, not one at a time

The unsolved piece. Render-prop-returns-data + `.map` works, but writing `<Target/>` per item is the
"one by one" problem. The fix: **a node's child set is the flattening of all its children, and a
child can be an *expander* that yields many nodes from a source + a template.** So you define the
*shape once* and a *source*, and the framework expands.

```jsx
<repo>
  {/* a literal item */}
  <File path="LICENSE" />

  {/* a PATTERN: source (enumeration) + template (what each item looks like), expanded */}
  <Each from={sh`git ls-files '*.rs'`}>
    {(path) => <HasHeader path={path} />}        {/* render prop is the TEMPLATE, applied per item */}
  </Each>

  {/* sugar for a glob source */}
  <Match glob="src/**/*.ts" as={(path) => <NoConsoleLog path={path} />} />
</repo>
```

`Each`/`Match` are just components that **return an array of nodes** — JSX already flattens arrays
and fragments, so "patterns" need no new runtime concept; they're an expander *stdlib* over the
existing tree. The render prop is used as a **template** (applied across a source), not as a
one-shot data-passer — that's the distinction that removes the one-by-one limitation.

### Mixing patterns and literals (and a deferred question)

A set-forming node can carry *both* a pattern and hardcoded items — your "render prop plus its own
children with vaguer rules plus hardcoded items." Children flatten into one keyed set, so a literal
and a pattern-expanded item sit side by side:

```jsx
<Files>
  <Match glob="src/**/*.ts" as={(p) => <NoConsoleLog path={p} />} />   {/* the general rule */}
  <NoConsoleLog path="src/special.ts" /* …extra config… */ />          {/* an extra hardcoded item */}
</Files>
```

What happens on a **key collision** (a literal with the same key as a pattern-expanded item), and
whether that is the right mechanism for **exemptions**, is deliberately **out of v1.** Last-wins
override is one possibility, but the "magic override / exemption" semantics are still vague and we
won't bake an unproven design into the base. **v1:** children union into a set; a collision is an
error (or last-wins — TBD); **exemptions are explored separately, not a core feature.**

### The set vocabulary (small, conventional)

- **literal** — `<Target …props />`, one item.
- **`<Each from={source}>{template}</Each>`** — map a template over an item stream.
- **`<Match glob="…" as={template} />`** — sugar: glob is the source.
- (future expanders are just more components that return node arrays: `<ForRepo>`, `<ForExport>`, …)
- collisions resolve **last-wins by `key`** → pattern-then-override.

A "set" is therefore not a special component — it's *any supervisor node's children*, where children
may be literals or expanders. The set-forming node is whatever owns them (`<repo>`, `<folder>`, a
user component).

## How it all fits the tree

```
<repo>                                    supervisor (has children)
  <RateLimited pool="agents">             decorator: injects Context into the subtree
    <Each from={exports}>                 expander: source + template → many items
      {(fn) => <Documented target={fn}/>} target instance (extendTarget(Assert)); parameterized by props
    </Each>
    <Documented target={special} exempt/> literal override (same key → wins → exempt)
  </RateLimited>
  <folder path="dist/">                   supervisor automatically (has children)
    <Match glob="dist/*.js" as={Minified}/>
  </folder>
</repo>
```

Down the tree: context + the act of reconciling children. Up: `Outcome`. Sideways: nothing but the
shared world. (See `agentic-codebase-substrate.md` §"Interfaces & composition".)

## Context & prompt grounding

A prompt generated deep in the tree needs grounding — *what repo, what package, what conventions*.
That is **the down-channel** the tree already has (context flows down, `Outcome` up); we just make it
author-facing and aim it at prompts. It is **React Context, scoped to prompts.**

Two kinds of context flow down, accumulated **top-down (general → specific)** at the point a prompt
is generated:

- **Structural — automatic.** cwd (from `<Dir path>`), the key/path chain, the kind. Short; the tree
  already knows it; **inline** it. The author never writes "we're in packages/core/src/insane.tsx."
- **Semantic — authored.** A `context=` prop (a.k.a. `data-context`) of *prose* ("packages/core is the
  ONLY published package; zero runtime deps; named exports only"). May be a literal **or computed from
  props** (`context={pkgSummary(name)}`) — it's just a JSX expression.

A prompt at node X is **auto-prepended** with the accumulated chain (root → … → X), then the node's own
prompt. Declared once per scope, inherited by everything beneath — the second payoff of nesting (the
first being dependency/ordering).

### Heavy/shared context → content-addressed files (read once)
To avoid re-inlining long prose into every prompt of a fan-out, materialize a semantic context entry
**once** to a **content-addressed** file and reference it by path:

```
.esto/context/<sha256(content)>.md      # gitignored, idempotent, pruned per run
```

Content-addressing is what makes "already seen it" real: identical context → identical path, so the
repo-level blurb shared by 40 prompts is **one** file, written once, referenced 40 times. A prompt then
carries `path + a one-line summary`, not the body:

```
Context (read once; same id across tasks = same content):
  .esto/context/ab12.md — insane-forms repo conventions
  .esto/context/9f3c.md — packages/core: published, zero-dep
Task: add a concise JSDoc to `field` …
```

The dedup pays off best under the **one-orchestrator-fans-out** model: the orchestrator loads each
context file once and recognizes the same id across every sub-task (prompt-cache-friendly). Rule of
thumb: **inline short + always-needed; content-addressed-ref for long + shared.**

It's one consumer of a general context map: `context` (prose) is the key *prompt generation* reads; cwd
is the key `sh` reads; an agent pool is the key dispatch reads — one channel, several consumers.

**Open:** dedup at the orchestrator (deterministic) vs relying on the agent to recognize the id; the
inline-vs-ref threshold; single-string `context=` vs typed providers (React-Context-style); a token
budget / priority when the chain gets long (append by default, allow a node to *override* rather than
only append).

## Cost & prioritization (per-node-type concurrency)

The footgun is putting expensive work in per-item hooks (a `value` that spawns a network call per
item → N calls per tick). Two cost phases, two disciplines:

- **Diff phase (`value` / `holds`) — cheap and batched.** Compute expensive fingerprints *once* in
  the `from` / `augment` pipeline (one batched enumeration) and pass them as props, so per-item
  `holds`/`value` are field reads. The API makes pre-computed props the path of least resistance and
  per-item thunks the explicit, discouraged exception.
- **Action phase (`fix`) — gated by a concurrency budget, not forbidden.** Expensive *actions* are
  fine; the system bounds them. **Each node type carries metadata: how many of its nodes may be under
  reconciliation at once** (e.g. `concurrency: 4` for an agent-backed assertion). When the budget is
  exhausted, remaining violations **defer to the next tick** — and because the loop is level-triggered,
  they resurface automatically until they get a slot. A `<RateLimited>` decorator can set the same
  budget for a subtree or a shared pool.

So prioritization is not "don't do expensive things"; it's **"declare how many at once, and let
level-triggering drain the queue across ticks."** Bounded concurrency + retry (the Erlang/HPA lesson)
as per-type metadata.

## Open questions

- **Template identity / keys.** An expander must produce stable `key`s so a rename is exit+enter on
  the *right* item, not churn — keys come from the source item, never list position.
- **Exemptions & key-collision override — DEFERRED (not in v1).** Whether a same-key literal
  overrides a pattern-expanded item, and how exemptions work at all, is unresolved and intentionally
  left out of the base. Explore separately; don't bake an unproven "magic override" in.
- **Decorator ordering / conflicts.** Nested decorators compose their Contexts; define the rule
  (innermost wins? merge?) when two decorators set the same context key.
- **`defineTarget` vs `extendTarget` boundary.** Almost everything is `extendTarget(Assert, …)`;
  `defineTarget` from scratch is for genuinely new lifecycle shapes (a process, a file, a window).
- **Concurrency-budget placement.** Per-type metadata vs a `<RateLimited>` decorator vs both — and
  whether deferred items need fairness/priority beyond "resurface next tick" (see *Cost & prioritization*).
