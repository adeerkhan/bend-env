# bend-env

A project and environment manager for [Bend 2](https://bend-lang.com): a
manifest, a lockfile and a pinned package environment, over
[BendHub](https://hub.bend-lang.com).

It is written in Rust and takes its shape from
[uv](https://github.com/astral-sh/uv): one binary, a global
content-addressed cache, parallel downloads, and an environment made of
hard links into that cache, so an install writes no new bytes and a
second project that wants the same package pays nothing.

```console
$ bendenv init my-model
wrote bend.toml for my-model

$ bendenv add bend-ml-tensor-fork
added bend-ml-tensor-fork * (0.1.2.0)
locked 2 packages, 3 files fetched, 4 installed

import it with the exact version the lock holds:

  import bend-ml-tensor-fork@0.1.2.0 /<entry>.bend as <Name>

$ bendenv tree
bend-ml-tensor-fork@0.1.2.0
  +- bend-ml-tensor-array@0.1.14.0

$ bendenv run -- bend main.bend
loss before:  2.4268
loss after:   1.8305869
```

## The three files

| File | What it is |
|---|---|
| `bend.toml` | the manifest: the project's name and its direct dependencies, each with a version constraint |
| `bend.lock` | the lockfile: every package in the closure, its exact version and its content hash, the packages each depends on, and the files of each with their digests |
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
| `bendenv init --lib [name]` | write a manifest for a package to publish, with a LICENSE |
| `bendenv init --workspace [name]` | write a manifest for a workspace of packages |
| `bendenv add <name>[@constraint]` | add a dependency, lock and sync |
| `bendenv add --dev <name>` | add a development dependency |
| `bendenv remove <name>` | drop a dependency, lock and sync |
| `bendenv lock` | resolve the manifest into `bend.lock` |
| `bendenv lock --check` | say whether the lock is current |
| `bendenv sync` | resolve and materialize `bend.lock` into `.bend/env` |
| `bendenv sync --locked` | materialize the lock as it is, without resolving |
| `bendenv list` | the packages the project resolves to |
| `bendenv tree` | the same, as a tree |
| `bendenv why <name>` | why a package is in the lock |
| `bendenv outdated` | the locked packages that have a newer version on the hub |
| `bendenv check` | verify the environment against the lock |
| `bendenv run -- <cmd>...` | run a command with the environment |
| `bendenv publish <name>@<version> <file.bend>` | publish a package to the hub |
| `bendenv cache clean` | empty the global cache |
| `bendenv cache dir` | where the global cache lives |
| `bendenv cache size` | how much the global cache holds |
| `bendenv purge` | delete the environment |

## Workspaces

A monorepo of packages is a workspace: a root manifest that names its
members, one lockfile for all of them, and one environment they share.

```toml
# bend.toml, at the root
[project]
name = "my-ml"

[workspace]
members = ["packages/*"]
```

```toml
# packages/preprocess/bend.toml
[project]
name = "mlbend-preprocess"
version = "0.1.0.0"

[dependencies]
bend-ml-nat-lemmas = "0.1.1.0"
bend-ml-tensor = "0.1.0.0"
```

`bendenv add` run inside a member writes that member's manifest and
re-locks the whole workspace; the lock and the environment live at the
root, and every member reads them. The lock holds one version of each
package for the whole workspace, so two members that want the same
dependency at different versions are a conflict, and the tool says so.

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

The one thing the resolver cannot do is make two versions of a package
be one type, and the tool says so when it happens — see
[the skew](#the-skew).

## The cache

`~/.cache/bendenv/files/` holds every file the hub has served, named by
its own SHA-256, so the store needs no index. The project's environment
is made of hard links into it: the same file in three packages is one
copy on disk with three links, and a second project that wants it
downloads nothing.

A file system that will not link (two mounts, say) gets a copy instead,
and the tool carries on.

Two kinds of thing are cached, and the difference matters. What a
package's *hash* names never changes — the files, the manifest, the
imports those files carry — so it is cached for good. What a package's
*name* names does change, because a new version can appear at any
moment, so that is cached for five minutes and then asked again.

### What it costs, measured

From a machine 4,000 km from the hub, whose round trip is 410 ms:

| | |
|---|---|
| the binary starts | 2 ms |
| `bendenv sync --locked` (no network) | 8 ms |
| `bendenv lock`, nothing cached | 8.3 s |
| `bendenv lock`, the cache warm | 1.9 s |

The cold number is the hub, not the tool: six requests, each paying its
own TLS handshake over a 410 ms round trip. The warm number is two
requests, for the two pages that move. The client asks each level of
the dependency walk at once, so a chain of four dependencies is two
round trips and not four.

## The collisions

A Bend type belongs to the package that declares it, so two packages
that both declare `Mat` are two different `Mat`, and a program that
passes one to the other does not compile. `bendenv` reports every type
two packages of the closure declare:

```
two packages of the closure declare the same type. A Bend type
belongs to the package that declares it, so they are two
different types, and a program that mixes them does not compile:
  G: bend-ml-autograd, bend-ml-tensor-array
  Mat: bend-ml-tensor, bend-ml-tensor-array
```

The ecosystem's flat tensor library and its list-based one are such a
pair, and so is the flat one and the autodiff that is written against
the other. The lock carries the type names of every package, so the
report needs no network.

## The skew

Two versions of one package can sit in a closure side by side. They are
two different types, and the checker refuses to mix them:

```
expected : 0xf9837737d2c3f58ae1d0c5df42255584/main.Mat<4n, 2n>
observed : T.Mat<4n, 2n>
```

That is the ecosystem's version skew, made visible: `bend-ml-autograd`
imports `bend-ml-tensor@0.1.0.0`, so a project that pins another version
holds both. The tool reports it on every `add` and `list`. It is a real
constraint, not a bug: two packages that share a type must share its
version.

## Integrity

Every file the hub serves is checked against the SHA-256 the hub
published in the package's manifest, on the way in and again by
`bendenv check`. A published version never moves: the hash is the
identity of the package, so a lockfile pins the bytes, not just the
number. `bendenv check` reads the lock alone, so it needs no network.

## The publish path

`bendenv publish <name>@<version> <file.bend>` sends a package to the
hub, and `bendenv hash <file.bend>` says what it would publish under,
without publishing anything:

```console
$ bendenv hash bendtorch.bend
bendtorch: 0x1689e5deb278c16abd3bd45eac101ef3
196 files, 4361634 bytes
```

That hash is the one the hub holds for `bendtorch-core@0.1.0.0`, over
the same 196 files: the file set, the paths, the digests and the hash
are the compiler's, byte for byte. The hub's proof of work is mined
over the cores, the way the compiler's own publisher does it.

## What it does not do yet

- **Versionless imports.** `import bend-ml-tensor-array/main.bend` would
  read the version from the lockfile, so a project would not have to
  repeat it in every file. The loader's `name_hash` would have to look
  in the lock instead of only in the hub; that is a change to the
  compiler, and it belongs upstream in `bendlang/bend`, not here. Until
  then, `bendenv add` prints the exact import line to paste.
- **A registry of the project's own packages.** A project that wants to
  publish a library it depends on still uses `bend --publish` directly.

## The checks

```console
cargo test          # 41 tests: the constraints and the files offline, the hub online
cargo clippy        # the lints
cargo fmt --check   # the format
```

The offline tests cover the version constraints, the import scanner, the
two file formats and the environment's verification. The online tests
talk to the hub: they resolve a package that has a dependency, take the
version a constraint names, and find the skew.

## The example

[`examples/mlp`](examples/mlp) is a two-layer neural network, trained in
Bend, with its packages coming from an environment this tool resolved and
locked. The loss after each step matches NumPy to the last digit of the
`F32` ([`reference.py`](examples/mlp/reference.py)).

## License

MIT.
