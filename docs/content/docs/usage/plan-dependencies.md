---
title: Plan dependencies and loop outcomes
weight: 47
---

Plan inspects resources without executing arbitrary commands. A command's
`changed_when` expression controls reporting; `changed_when: false` does not
prove that the command is safe to execute during plan.

```yaml
- !command
  name: Read current setting
  cmd: /usr/bin/printf enabled
  save: current_setting
  changed_when: false

- !command
  name: Converge setting
  cmd: /usr/bin/true
  with:
    condition: '(not (contains current_setting.out "enabled"))'
```

During plan the first task reports `would run`, and the dependent task reports
`deferred: depends on output unavailable until apply`. Neither command executes.
The recap has a separate **Deferred** count and reports an incomplete plan,
rather than claiming that every host matched policy. A deferred outcome is
neither a failure, a known change, an empty result, nor a matched resource.
Plan can finish successfully while reporting these unknown outcomes.

Unavailable output bindings are tracked separately from actual saved host
variables. Previously saved values cannot stand in for skipped producers.
Tracking is scoped to each host; one host or item cannot overwrite another
host's availability. Known templated save names are resolved per invocation.
Actual assignments clear the corresponding dependency, including passive tasks
that execute normally during plan. Normal apply still executes commands and
saves their actual results.

The shared condition evaluator checks unavailable values only when an operand
is reached, preserving lazy `and` and `or`. For example, `(and false (eq
current_setting.out "enabled"))` is false without reading the unavailable value.
`isdefined` on an unavailable result is deferred, rather than treating an
unknown future value as absent. Ordinary undefined variables and malformed
conditions remain errors.

Dependent module parameters, iteration sources, aliases, and downstream saved
results also propagate deferral. Template dependency checks are conservative:
a reference to unavailable data in a template can defer it even if a template
branch would not use that reference. Dynamic `lookup` into the root context
checks the selected key at render time. No result is fabricated to make the
condition pass or fail.

Prefer native resource queries and HTTP preflight `check.matches` when they
express the desired state. Those inspect state through their established plan
behavior. There is no implicit plan-time command execution or new read-only
command assertion in this change.

## Item loops

Each task declaration produces one aggregate result per host. Iteration still
executes one item at a time; package batching and saved-result semantics are
unchanged.

- Any item needing work makes the plan report work, regardless of item order.
- Any changed item makes apply report changed, including when later items match
  or skip. Standard handler notifications retain changes from earlier items.
- All matched items report matched. Matched plus skipped reports matched;
  all skipped reports skipped. Passive results remain unchanged.
- Different change categories combine as modification, with the union of changed
  fields. Failures retain normal immediate-stop/retry behavior; ignored failures
  remain visible in the aggregate.
- Mixed known changes and deferred items retain both facts. The task reports the
  known work and its partial deferral; its Deferred recap count can overlap the
  change count. Purely deferred tasks do not count as unchanged.

A loop's saved output continues to reflect normal per-item assignments; Jetpack
also avoids presenting the final item's command output as though it described
the entire aggregate task.
