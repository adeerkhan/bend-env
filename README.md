# bend-env

A project and environment manager for [Bend 2](https://bend-lang.com): a
manifest, a lockfile and a pinned package environment, over
[BendHub](https://hub.bend-lang.com).

`pip` installs a package into one interpreter's site-packages. `uv` adds a
project: a manifest, a lockfile and a per-project environment. Bend needs
the same three things, for the same reasons: a program depends on
packages, the versions it was written against must not move under it, and
two projects that need different versions of the same package must be able
to sit side by side.

```console
$ bendenv init my-model
wrote bend.toml for my-model

$ bendenv add bend-ml-tensor-fork
added bend-ml-tensor-fork * (0.1.2.0)
locked 2 packages, 4 files fetched

import it with the exact version the lock holds:

  import bend-ml-tensor-fork@0.1.2.0/<entry>.bend as <Name>

$ bendenv tree
bend-ml-tensor-fork@0.1.2.0
  +- bend-ml-tensor-array@0.1.14.0

$ bendenv run -- bend main.bend
[12, 12, 12, 12]
```

## The three files

| File | What it is |
|---|---|
| `bend.toml` | the manifest: the project's name and its direct dependencies, each with a version constraint |
| `bend.lock` | the lockfile: every package in the closure, its exact version and its content hash, and what each depends on |
| `.bend/env/` | the environment: the packages themselves, verified against the hashes the hub published |

The environment is the shape Bend already uses for its own cache,
`~/.bend/lib`: a `names` directory that maps a pinned name to a hash, and
one directory per hash holding that package's files. Pointing `BEND_LIB`
at it makes the loader read it and nothing else, so an environment is a
package set, pinned and verified — and `bendenv run` is that one
environment variable.

## The commands

| Command | What it does |
|---|---|
| `bendenv init [name]` | write a manifest |
| `bendenv add <name>[@constraint]` | add a dependency, lock and sync |
| `bendenv remove <name>` | drop a dependency, lock and sync |
| `bendenv lock` | resolve the manifest into `bend.lock` |
| `bendenv sync` | materialize `bend.lock` into `.bend/env` |
| `bendenv list` | the packages the project resolves to |
| `bendenv tree` | the same, as a tree |
| `bendenv why <name>` | why a package is in the lock |
| `bendenv outdated` | the locked packages that have a newer version on the hub |
| `bendenv check` | verify the environment against the lock |
| `bendenv run -- <cmd>...` | run a command with the environment |
| `bendenv purge` | delete the environment |

## Why there is no conflict resolution

A package manager that has to choose between two versions of the same
dependency needs a solver. This one does not, and the reason is Bend's
own import rule.

A Bend source names its dependencies exactly:

```python
import bend-ml-tensor-array@0.1.14.0/main.bend as TA
```

The loader resolves that name and version to a hash, and two versions of
one package live in two directories and never meet. So the dependency
graph of a project is a set, not a choice: the resolver only walks it.

That also means the dependencies of a published package are *in its
source*, not in a manifest field the hub has to understand. The resolver
reads the import lines of a package's files and follows them. No change
to the hub's format is needed for transitive resolution to work, and a
package cannot lie about what it depends on.

## Integrity

Every file the hub serves is checked against the SHA-256 the hub
published in the package's manifest, both on the way in and by
`bendenv check`. A published version never moves: the hash is the
identity of the package, so a lockfile pins the bytes, not just the
number.

## What it does not do yet

- **Versionless imports.** `import bend-ml-tensor-array/main.bend` would
  read the version from the lockfile, so a project would not have to
  repeat it in every file. The loader's `name_hash` would have to look
  in the lock instead of only in the hub; that is a change to the
  compiler, and it belongs upstream in `bendlang/bend`, not here. Until
  then, `bendenv add` prints the exact import line to paste.
- **Publishing.** `bend --publish` already does it, with the hub's proof
  of work. This tool does not wrap it.
- **A registry of the project's own packages.** A project that wants to
  publish a library it depends on still uses `bend --publish` directly.

## The checks

```console
bun test          # 38 tests: the constraints and the files offline, the hub online
bunx tsc --noEmit # the types
```

The offline tests cover the version constraints, the import scanner and
the two file formats. The online tests talk to the hub: they resolve a
package that has a dependency, materialize an environment, corrupt a file
and find it again.

## License

MIT.
