---
title: Template fragments and validation
weight: 45
---

A file template can insert sorted directories of fragments at any location:

```handlebars
[frontend]
{{{ include_dir "../fragments/frontend" }}}
[backends]
{{{ include_dir "../fragments/backends" }}}
```

Paths are controller-local, relative to the containing template or fragment.
The active playbook directory (or role directory while executing a role) is the
permitted source root. An explicit absolute `src` outside that root permits its
own containing directory instead. Includes cannot escape that root, including
through symlinks. Organize sibling `templates/` and `fragments/` directories
inside the same playbook or role. Nothing implicitly permits the whole checkout
or arbitrary controller paths.

Regular non-hidden files are read in filename order, independent of locale.
Subdirectories are skipped; use another explicit `include_dir` to include them.
Unsupported objects such as FIFOs fail before reading. Empty directories insert
nothing; missing or unreadable directories fail. Canonical paths detect cycles,
with a maximum nesting depth of 64. Do not modify source trees during a run.

Each fragment uses the calling template's variables, including host values and
`each`/`with` scope. Fragment output is raw text even with double braces; a final
newline is added to nonempty fragments that lack one. Composed templates render
once: output containing `{{ ... }}` is literal output, not another evaluation.
Standalone templates retain their existing recursive variable expansion.
Adding or removing a fragment takes effect on the next render.

## Validate the complete candidate

```yaml
- !template
  src: service.conf.hb
  dest: /etc/example/service.conf
  validate: 'exampled --check --config {}'
  chdir: /etc/example
  attributes:
    mode: '0640'
  and:
    notify: restart example
```

`validate` is an optional templated command parsed into arguments. Exactly one
argument must be `{}`; `--config={}` is invalid. Jetpack replaces that argument
with the candidate's absolute target-side path and quotes every argument.
Shell pipelines, expansions, and redirections are unsupported. A candidate path
containing spaces is still one argument. Validators should only inspect the
candidate and return zero on success. Validator stdout/stderr is withheld to
avoid exposing configuration secrets; failures report the exit status.

The selected execution connection, delegation, sudo settings, and optional
absolute target-side `chdir` apply to staging and validation. The candidate is
created beside the destination with mode `0600`. The validator can access other
resources on that target. Validation failure leaves live contents and metadata
untouched and does not notify handlers. Metadata overrides are applied to the
candidate after validation. Existing owner, group, and mode are preserved unless
specified; new files default to `0600` and the effective task user/group.

All template installs use this transaction, including templates without a
validator. Destination symlinks and nonregular files are rejected; existing
ancestor directories follow the target's normal path semantics. Relative
`dest` paths retain target-side directory semantics. Parent directories must
already exist. Candidates are promoted by a same-filesystem atomic rename and
cleaned up after success or failure while the connection remains available.
A private upload directory is removed as well.

Jetpack compares the source contents and file metadata again before promotion
and refuses an observed concurrent change. This is optimistic detection, not a
lock or compare-and-swap: an uncooperative writer can race the final check or
write again after replacement. Atomic rename prevents partial installed files.
ACLs, extended attributes, and hard-link relationships are not managed by this
transaction.

`plan` renders and inspects current state, but creates no candidate, invokes no
validator, and writes nothing. It cannot promise that a future validator will
accept the candidate. Unchanged reapply does not stage, validate, replace, or
notify.
