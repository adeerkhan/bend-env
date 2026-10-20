# A two-layer MLP, trained with the packages of the environment.
#
# The shapes are in the types: x is Mat<4n, 2n>, the hidden layer is Mat<4n, 3n>,
# the output is Mat<4n, 1n>. A layer with the wrong width does not compile,
# and the gradient of every layer has the shape of that layer.
#
# The model is a value, not an object with state: a Weights record is passed
# to a step and comes back a new one. That is Bend's affine rule making the
# mutating shape impossible.
#
#   bendenv run -- bend main.bend
import Base
import bend-ml-tensor@0.1.0.0/main.bend as T
import bend-ml-autograd@0.1.1.0/main.bend as AG

# The fitted model: four tensors, each with its shape in its type.
type Weights is Data:
  W{+w1: T.Mat<2n, 3n>, +b1: T.Vec<3n>, +w2: T.Mat<3n, 1n>, +b2: T.Vec<1n>}

# The gradients of the two layers, in the shape of the layers they belong to.
type OutGrads is Data:
  OG{+dh: T.Mat<4n, 3n>, +dw2: T.Mat<3n, 1n>, +db2: T.Vec<1n>}

type InGrads is Data:
  IG{+dx: T.Mat<4n, 2n>, +dw1: T.Mat<2n, 3n>, +db1: T.Vec<3n>}

# ---- the data: four samples, two features, one target each ----

def x_rows() -> List<&2, List<&2, F32>>:
  [[1.0, 0.0], [0.0, 1.0], [1.0, 1.0], [0.5, 0.5]]

def t_rows() -> List<&2, List<&2, F32>>:
  [[1.0], [1.0], [1.0], [1.0]]

def xs.maybe() -> Maybe<&2, T.Mat<4n, 2n>>:
  T.Mat.of(4n, 2n, x_rows())

def xs.of(m: Maybe<&2, T.Mat<4n, 2n>>) -> T.Mat<4n, 2n>:
  match m:
    case Some{v}:
      v
    case None{}:
      # the rows are four of two, so Mat.of cannot fail here
      T.Mat.zeros(4n, 2n)

def xs() -> T.Mat<4n, 2n>:
  xs.of(xs.maybe())

def ts.maybe() -> Maybe<&2, T.Mat<4n, 1n>>:
  T.Mat.of(4n, 1n, t_rows())

def ts.of(m: Maybe<&2, T.Mat<4n, 1n>>) -> T.Mat<4n, 1n>:
  match m:
    case Some{v}:
      v
    case None{}:
      # the rows are four of one, so Mat.of cannot fail here
      T.Mat.zeros(4n, 1n)

def ts() -> T.Mat<4n, 1n>:
  ts.of(ts.maybe())

# ---- the starting weights ----

def start() -> Weights:
  W{T.Mat.fill(2n, 3n, 0.2), T.Vec.fill(3n, 0.0),
    T.Mat.fill(3n, 1n, 0.3), T.Vec.fill(1n, 0.0)}

# ---- the forward pass ----

def pre(x: T.Mat<4n, 2n>, w1: T.Mat<2n, 3n>, b1: T.Vec<3n>) -> T.Mat<4n, 3n>:
  AG.linear(4n, 2n, 3n, x, w1, b1)

def hidden(p: T.Mat<4n, 3n>) -> T.Mat<4n, 3n>:
  T.Mat.relu(4n, 3n, p)

def out(h: T.Mat<4n, 3n>, w2: T.Mat<3n, 1n>, b2: T.Vec<1n>) -> T.Mat<4n, 1n>:
  AG.linear(4n, 3n, 1n, h, w2, b2)

def predict(x: T.Mat<4n, 2n>, m: Weights) -> T.Mat<4n, 1n>:
  match m:
    case W{w1, b1, w2, b2}:
      out(hidden(pre(x, w1, b1)), w2, b2)

# ---- the loss: the sum of the squares of the errors ----

def errors(y: T.Mat<4n, 1n>, t: T.Mat<4n, 1n>) -> T.Mat<4n, 1n>:
  T.Mat.sub(4n, 1n, y, t)

def loss_of(+e: T.Mat<4n, 1n>) -> F32:
  T.Vec.sum(1n, T.Mat.col_sums(4n, 1n, T.Mat.mul(4n, 1n, e, e)))

def loss(x: T.Mat<4n, 2n>, t: T.Mat<4n, 1n>, m: Weights) -> F32:
  loss_of(errors(predict(x, m), t))

# ---- the backward pass ----

# d(loss)/d(error) = 2 * error
def dy_of(e: T.Mat<4n, 1n>) -> T.Mat<4n, 1n>:
  T.Mat.scale(4n, 1n, 2.0, e)

# the output layer, backward
def out_bwd(h: T.Mat<4n, 3n>, w2: T.Mat<3n, 1n>, dy: T.Mat<4n, 1n>) ->
  T.Mat<4n, 3n> & (T.Mat<3n, 1n> & T.Vec<1n>):
  AG.linear_bwd(4n, 3n, 1n, h, w2, dy)

def og_of(r: T.Mat<4n, 3n> & (T.Mat<3n, 1n> & T.Vec<1n>)) -> OutGrads:
  (dh, (dw2, db2)) = r
  OG{dh, dw2, db2}

def og.dh(g: OutGrads) -> T.Mat<4n, 3n>:
  OG{dh, dw2, db2} = g
  dh

def og.dw2(g: OutGrads) -> T.Mat<3n, 1n>:
  OG{dh, dw2, db2} = g
  dw2

def og.db2(g: OutGrads) -> T.Vec<1n>:
  OG{dh, dw2, db2} = g
  db2

# the hidden layer's gradient, through the relu
def dz_of(p: T.Mat<4n, 3n>, dh: T.Mat<4n, 3n>) -> T.Mat<4n, 3n>:
  AG.relu_bwd(4n, 3n, p, dh)

# the input layer, backward
def in_bwd(x: T.Mat<4n, 2n>, w1: T.Mat<2n, 3n>, dz: T.Mat<4n, 3n>) ->
  T.Mat<4n, 2n> & (T.Mat<2n, 3n> & T.Vec<3n>):
  AG.linear_bwd(4n, 2n, 3n, x, w1, dz)

def ig_of(r: T.Mat<4n, 2n> & (T.Mat<2n, 3n> & T.Vec<3n>)) -> InGrads:
  (dx, (dw1, db1)) = r
  IG{dx, dw1, db1}

def ig.dw1(g: InGrads) -> T.Mat<2n, 3n>:
  IG{dx, dw1, db1} = g
  dw1

def ig.db1(g: InGrads) -> T.Vec<3n>:
  IG{dx, dw1, db1} = g
  db1

# ---- one step of SGD ----

def step(+x: T.Mat<4n, 2n>, +t: T.Mat<4n, 1n>, m: Weights, +lr: F32) -> Weights:
  match m:
    case W{w1, b1, w2, b2}:
      +p = pre(x, w1, b1)
      +h = hidden(p)
      e = errors(out(h, w2, b2), t)
      +g2 = og_of(out_bwd(h, w2, dy_of(e)))
      dz = dz_of(p, og.dh(g2))
      +g1 = ig_of(in_bwd(x, w1, dz))
      W{AG.sgd(2n, 3n, lr, w1, ig.dw1(g1)), AG.sgd_vec(3n, lr, b1, ig.db1(g1)),
        AG.sgd(3n, 1n, lr, w2, og.dw2(g2)), AG.sgd_vec(1n, lr, b2, og.db2(g2))}

# The loss before the first step, and after each of the next three.
def report(+x: T.Mat<4n, 2n>, +t: T.Mat<4n, 1n>) -> String:
  +m0 = start()
  l0 = loss(x, t, m0)
  +m1 = step(x, t, m0, 0.01)
  l1 = loss(x, t, m1)
  +m2 = step(x, t, m1, 0.01)
  l2 = loss(x, t, m2)
  +m3 = step(x, t, m2, 0.01)
  l3 = loss(x, t, m3)
  String.append(
    String.append(
      String.append(
        String.append("loss before:  ", F32.show(l0)),
        String.append("\nloss after:   ", F32.show(l1))),
      String.append("\nloss after:   ", F32.show(l2))),
    String.append("\nloss after:   ", F32.show(l3)))

def main() -> IO(Unit):
  IO.print(report(xs(), ts()))
