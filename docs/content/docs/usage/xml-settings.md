---
title: Edit selected XML settings
weight: 46
---

`!xml` manages selected elements while preserving unrelated configuration:

```yaml
- !xml
  path: /etc/example/network.xml
  create: true
  set:
    /NetworkConfiguration/EnableHttps: false
    /NetworkConfiguration/HttpServerPortNumber: '{{ application_port }}'
    /NetworkConfiguration/KnownProxies:
      children:
        string: '{{ trusted_proxy_addresses }}'
  remove:
    - /NetworkConfiguration/ObsoleteSetting
  and:
    notify: restart application
```

Whole-value variable templates preserve their actual types. For example,
`application_port: 8096` remains a number and `trusted_proxy_addresses` remains
a list, rather than a string representation of a list. Mixed text templates
remain strings. Variable expansion uses the normal invocation's host/item
context. Selectors, child element names, and namespace declarations are literal;
only expected values and the file path are templated.

## Selectors and explicit creation

Selectors are absolute paths of literal ASCII XML element names, optionally
prefixed, such as `/Configuration/Network/Port` or `/n:Configuration/n:Port`.
Supported name characters are letters, digits, underscore, hyphen, and period,
with a letter or underscore first. This is an intentionally narrow element-path
subset, not XPath: attributes, predicates, indexes, wildcards, descendant axes,
and functions are unsupported. Multiple matches fail; Jetpack never silently
chooses the first element. Overlapping selectors, including namespace aliases
for the same element, are rejected before writing.

`create` defaults to false. Missing files, ancestors, or selected elements fail
unless `create: true` explicitly permits their creation. This also permits new
repeated children when the desired list is longer. A new document's root is
inferred from its set selectors; existing document roots are never replaced.
Creating a file requires at least one set operation. Parent filesystem
directories must already exist. `path` is an absolute path in the execution
target's filesystem.

`remove` explicitly removes an element and its subtree; an absent element or
ancestor is unchanged. Removing the document root is unsupported. `null` is
never deletion and is rejected as a value.

## Scalar and repeated values

Strings become escaped XML text and compare exactly after XML entity decoding,
including whitespace. Booleans write `true` or `false`; existing `1`/`0` also
compare as true/false. Numbers write their JSON number spelling and compare by
parsing trimmed element text as a JSON number. Integer and floating-point
representations follow JSON value comparison. XML text itself has no JSON type:
the expected value's type selects the comparison. No schema or `xsi:type`
inference is performed.

Scalars may only select elements without element children. Existing comments,
processing instructions, and attributes on a scalar element are retained.
Changing text may replace CDATA with equivalent escaped text. Unchanged values
retain their original bytes, including CDATA, escaping, and numeric spelling.

Repeated elements use an explicit `children` mapping of element names to scalar
lists:

```yaml
set:
  /NetworkConfiguration/KnownProxies:
    children:
      string: [192.0.2.10, 192.0.2.11]
```

The parent must contain exactly that ordered list of children named `string`
after the edit. Other child names, attributes, comments, processing instructions,
and surrounding whitespace are retained. Existing selected children are updated
in place, preserving their attributes and comments; excess selected children are
removed and additional ones are inserted after the last selected child (or at
the parent's end). Empty lists remove all children of that name while retaining
the parent and other content. Several differently named lists may be managed;
their order relative to unrelated child names is not changed. Nested objects,
nulls, and arrays inside these scalar lists are unsupported.

## Namespaces

Declare prefixes explicitly and use them in selectors and child names:

```yaml
- !xml
  path: /etc/example/network.xml
  namespaces:
    n: urn:example:network
  create: true
  set:
    /n:NetworkConfiguration/n:KnownProxies:
      children:
        n:string: ['{{ proxy_address }}']
```

Prefixes resolve by namespace URI, so the document may use another prefix or a
default namespace. Unprefixed selectors mean **no namespace**, even when the
source document declares a default namespace. New elements receive necessary
namespace declarations; unrelated declarations and lexical prefixes are kept.

## Transaction and preservation guarantees

Jetpack parses and prepares every edit in memory before staging anything. The
XML parser rejects malformed documents and all DTDs; external entity resolution
and external resource fetching are disabled. Files must be UTF-8. Errors report
locations without printing configuration values or document excerpts.

Edits splice selected source ranges; they do not reserialize the whole document.
Unrelated content, comments, processing instructions, attributes, ordering, and
formatting remain byte-for-byte unchanged. Whitespace inside an explicitly
managed scalar is part of its value. Repeated-element removals leave surrounding
whitespace intact.

Reads, staging, and installation use the execution connection, delegation, and
task privileges. Existing owner, group, and mode are preserved unless explicit
`attributes` override them. New files default to `0600`. Destination symlinks and
nonregular files fail; ancestor directories follow the target's normal path
semantics. The shared [file transaction](../template-composition/) stages
restrictively beside the destination, cleans up candidates, and promotes by
atomic rename. ACLs, extended attributes, and hard-link relationships are outside
its metadata scope.

Observed changes to the original file during a task cause failure rather than
overwriting another writer. This is optimistic detection, not a lock against
uncooperative application writes: a writer can race the final check or change
the file after replacement. Atomic replacement prevents partial installed files.
Each new invocation reads the latest file, preserving application settings that
changed between applies while converging the selected fields.

`plan` parses, compares, and reports differences without staging or writing.
`apply` installs only when content or explicitly managed attributes differ.
Unchanged reapply does not replace the file or notify handlers. Restart/reload
behavior uses ordinary task notifications; the XML module has no service-specific
behavior.
