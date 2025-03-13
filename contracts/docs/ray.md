# Ray

"RAY" is a fixed point number unit we use in Angstrom to represent fractional numbers. It's very
similar to the widely used "WAD" but instead of 18 decimals it has 27.

For example:

```
Number = RAY representation
1.0 = 1000000000000000000000000000
2.33 / 13 = 179230769230769230769230769
```

Mathematically to see how a given operation is implemented consider that the representation $r$ of a given $x \in \mathbb{R}$ is computed as: $r = \lfloor x \cdot 10^{27} \rfloor$

So e.g. if you have a $r_x$ and $r_y$ and wanted to compute $r_z$ such that $z = x \cdot y$ it'd end up
simplifying to $r_z = \lfloor \frac{r_x \cdot r_y}{10^{27}} \rfloor$
