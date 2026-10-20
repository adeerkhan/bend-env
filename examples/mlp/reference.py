# The numbers Bend gets, checked against NumPy.
#
#   python3 reference.py
#
# The same four samples, the same starting weights, the same learning
# rate and the same order of operations as main.bend: the loss before
# the first step, and after each of the next three.
#
# F32 is not a real number, so the numerics are tested and not proved.
# What is proved is the shape: every gradient has the shape of the
# layer it belongs to, and the checker refuses the rest.

import numpy as np

X = np.array([[1.0, 0.0], [0.0, 1.0], [1.0, 1.0], [0.5, 0.5]], dtype=np.float32)
T = np.array([[1.0], [1.0], [1.0], [1.0]], dtype=np.float32)

LR = np.float32(0.01)


def loss(x, w1, b1, w2, b2):
    h = np.maximum(x @ w1 + b1, 0)
    y = h @ w2 + b2
    e = y - T
    return float((e * e).sum())


def main() -> None:
    w1 = np.full((2, 3), 0.2, dtype=np.float32)
    b1 = np.zeros(3, dtype=np.float32)
    w2 = np.full((3, 1), 0.3, dtype=np.float32)
    b2 = np.zeros(1, dtype=np.float32)

    print("loss before: ", loss(X, w1, b1, w2, b2))
    for _ in range(3):
        h = np.maximum(X @ w1 + b1, 0)
        y = h @ w2 + b2
        e = y - T
        dy = 2 * e
        dh = dy @ w2.T
        dw2 = h.T @ dy
        db2 = dy.sum(axis=0)
        dz = dh * (h > 0)
        dw1 = X.T @ dz
        db1 = dz.sum(axis=0)
        w1 = w1 - LR * dw1
        b1 = b1 - LR * db1
        w2 = w2 - LR * dw2
        b2 = b2 - LR * db2
        print("loss after:  ", loss(X, w1, b1, w2, b2))


if __name__ == "__main__":
    main()
