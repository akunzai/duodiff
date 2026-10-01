# A scan takes its roots and rules as arguments

**Status**: accepted

## Context

`diff::align_directories` takes seven arguments and `scan::start` eight, each
under `#[allow(clippy::too_many_arguments)]`. An architecture review proposed
bundling the two roots and their ignore rules into one parameter object.

## Decision

Keep the arguments as they are.

- Production has one call into `align_directories`, from `scan::start`; a full
  scan and a one-directory rescan differ only in the `path` they pass. The
  tests reach it through `align_directories_with_shared_matcher`, which is the
  narrower entry they need.
- A struct holding `left`, `right`, and their two matchers moves those four
  arguments without hiding any behaviour: the caller still builds it and the
  scan still unpacks it, so the interface is no deeper (deletion test).
- `scan::start` was already narrowed to its inputs rather than the whole `App`
  (#381), which is where the real coupling was.

## Consequences

- The `too_many_arguments` allows stay. They are not a finding.
- Revisit when a scan can come from a source other than two local directories,
  such as an archive or a remote root. Then the roots vary behind a seam, and a
  type that owns how to list one is worth having.
