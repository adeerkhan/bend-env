# The MLP example

A two-layer neural network, trained in Bend, with its packages coming
from an environment this tool resolved and locked.

```console
$ bendenv sync
$ bendenv run -- bend main.bend
loss before:  2.4268
loss after:   1.8305869
loss after:   1.3619208
loss after:   1.0017024
```

The same four numbers come out of NumPy (`python3 reference.py`), to the
last digit of the `F32`.

## What it shows

**The shapes are in the types.** `x` is `Mat<4n, 2n>`, the hidden layer
is `Mat<4n, 3n>`, the output is `Mat<4n, 1n>`, and the gradient of every
layer has the shape of the layer it belongs to. A layer with the wrong
width does not compile, and neither does a gradient with the wrong one.

**The model is a value.** `Weights` is a record of four tensors; a step
takes one and returns another. There is no object with mutated state,
because Bend's affine rule makes that shape impossible.

**The packages are pinned.** `bend.lock` holds the exact version and the
content hash of every package, and `bend-ml-autograd` brings its own
dependency with it. The lock carries the file list too, so the
environment can be verified without the network.

## A note on the versions

`bend-ml-autograd@0.1.1.0` imports `bend-ml-tensor@0.1.0.0`, so this
project pins the same `bend-ml-tensor`. Pinning a different one does not
fail at the tool's level — two versions of a package sit side by side
happily — but the two are *different types*, and the checker refuses to
mix them:

```
expected : 0xf9837737d2c3f58ae1d0c5df42255584/main.Mat<4n, 2n>
observed : T.Mat<4n, 2n>
```

That is the ecosystem's version skew, made visible. It is a real
constraint, not a bug in the tool: two packages that share a type must
share its version.
