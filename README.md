# Stable names for pangenome graphs

This is a proposal for generating stable names for pangenome graphs.
The names are SHA-256 hashes of a canonical GFA representation of the graph.

See [refget](https://ga4gh.github.io/refget/) for a similar naming scheme for sequences.

## Intended applications

* Tagging various indexes with the name of the corresponding graph.
* As a reference name in a read alignment file.
* For representing relationships such as "A is a subgraph of B" or "A can be translated to B".
* For checking whether two graphs are the same, up to the node identifiers.
    * If A is a subgraph of B, graph B can be used as a reference with reads aligned to A.
    * Some tools chop long nodes to smaller fragments, but coordinates in the chopped graph can be translated to the original coordinates.

## Example

We have three graphs:

* `original.gfa`: The original graph with some long nodes.
* `translated.gbz`: The same graph, with long nodes chopped into 1024 bp fragments.
* `sampled.gbz`: A personalized graph sampled from `translated.gbz`.

These graphs have the following names:

```txt
1f133f116e8dd98fc07a647a8954038c2bcf07a45759ba94718471fe34ed7a7c  original.gfa
e10f3b362d8a4273059d9aea38a78bd71913418c3f3c9a2b5ea44e86de2c1181  translated.gbz
7f4b28c71ceb808aebd8b8e9fe85e79d0d208ee263ffe9fcdef5ade20534ceb5  sampled.gbz
```

We want to store the following information for `sampled.gbz`:

* The name of the graph.
* `sampled.gbz` is a subgraph of `translated.gbz`.
* Coordinates can be translated in both directions between `translated.gbz` and `original.gfa`.

### GBZ tags

```txt
pggname = 7f4b28c71ceb808aebd8b8e9fe85e79d0d208ee263ffe9fcdef5ade20534ceb5
subgraph = 7f4b28c71ceb808aebd8b8e9fe85e79d0d208ee263ffe9fcdef5ade20534ceb5,e10f3b362d8a4273059d9aea38a78bd71913418c3f3c9a2b5ea44e86de2c1181
translation = e10f3b362d8a4273059d9aea38a78bd71913418c3f3c9a2b5ea44e86de2c1181,1f133f116e8dd98fc07a647a8954038c2bcf07a45759ba94718471fe34ed7a7c;1f133f116e8dd98fc07a647a8954038c2bcf07a45759ba94718471fe34ed7a7c,e10f3b362d8a4273059d9aea38a78bd71913418c3f3c9a2b5ea44e86de2c1181
```

### GFA header

```txt
H	NM:Z:7f4b28c71ceb808aebd8b8e9fe85e79d0d208ee263ffe9fcdef5ade20534ceb5
H	SG:Z:7f4b28c71ceb808aebd8b8e9fe85e79d0d208ee263ffe9fcdef5ade20534ceb5,e10f3b362d8a4273059d9aea38a78bd71913418c3f3c9a2b5ea44e86de2c1181
H	TL:Z:e10f3b362d8a4273059d9aea38a78bd71913418c3f3c9a2b5ea44e86de2c1181,1f133f116e8dd98fc07a647a8954038c2bcf07a45759ba94718471fe34ed7a7c
H	TL:Z:1f133f116e8dd98fc07a647a8954038c2bcf07a45759ba94718471fe34ed7a7c,e10f3b362d8a4273059d9aea38a78bd71913418c3f3c9a2b5ea44e86de2c1181
```

### GAF header

```txt
@RN	7f4b28c71ceb808aebd8b8e9fe85e79d0d208ee263ffe9fcdef5ade20534ceb5
@SG	7f4b28c71ceb808aebd8b8e9fe85e79d0d208ee263ffe9fcdef5ade20534ceb5	e10f3b362d8a4273059d9aea38a78bd71913418c3f3c9a2b5ea44e86de2c1181
@TL	e10f3b362d8a4273059d9aea38a78bd71913418c3f3c9a2b5ea44e86de2c1181	1f133f116e8dd98fc07a647a8954038c2bcf07a45759ba94718471fe34ed7a7c
@TL	1f133f116e8dd98fc07a647a8954038c2bcf07a45759ba94718471fe34ed7a7c	e10f3b362d8a4273059d9aea38a78bd71913418c3f3c9a2b5ea44e86de2c1181
```

Here we use `RN` (reference name) instead of `NM` (name).

## Canonical GFA format

Sort the nodes by their identifiers.
Interpret node identifiers as integers, if possible, and fall back to strings if at least one of the identifiers is not an integer.

For each node, in sorted order, output:

* S-line for the node without optional fields.
* L-lines for all canonical edges, without the overlap field or optional fields, in sorted order.

The canonical GFA representation of the graph does not include any other information, such as header lines, paths, or walks.

### Technicalities

Each line is terminated by a single `\n`, and the fields in a line are separated by a single `\t`.
There are no empty fields or empty lines.
The content of each field must be as in valid GFA.
A node with an empty sequence therefore has no canonical GFA representation, though the graph
implementations in this crate tolerate one.

### Nodes (GFA segments)

The sequence label of each node must be stored explicitly.
Sequences are case sensitive, as some graph implementations do not normalize them.
Upper case sequences are strongly recommended.

### Edges (GFA links)

An edge is canonical, if the source id is smaller than the destination id.
A self-loop is canonical, if at least one of the nodes is in forward orientation.

Edges are sorted by (source orientation, destination id, destination orientation).
The forward orientation comes before the reverse orientation.

### Example

Consider the following example graph from the GFA specification, with overlaps changed to `0M`:

```txt
H	VN:Z:1.0
S	11	ACCTT
S	12	TCAAGG
S	13	CTTGATT
L	11	+	12	-	0M
L	12	-	13	+	0M
L	11	+	13	+	0M
P	14	11+,12-,13+	0M,0M
```

Its canonical GFA representation is:

```txt
S	11	ACCTT
L	11	+	12	-
L	11	+	13	+
S	12	TCAAGG
L	12	-	13	+
S	13	CTTGATT
```

And its stable name is:

```txt
54b49d18354a34fbd1af9aaac279e1b3ee67b2f68f0ff79f5ebf6c50c8d922a5
```

## Other versions

* Node identifiers interpreted as integers or strings.
    * The canonical order of the nodes depends on the type of the identifiers.
    * Using string identifiers requires more memory.
    * String identifiers are faster with GFA graphs and slower with GBZ graphs.
* All SHA-2 variants.

## Graph isomorphism

Two graphs have the same name only if their node identifiers agree.
Graphs that differ only in the identifiers therefore get different names, even though they
represent the same pangenome.
The `--compare` option answers that question directly.

Two graphs may also represent the same pangenome without being isomorphic as graphs, because one of
them has chopped long nodes into shorter fragments.
The comparison is therefore made at the level of maximal non-branching paths: each such path is
collapsed into a single node before the comparison.
Chopping a node only adds boundaries inside such a path, so the collapsed graphs are the same.
This is the relationship the `translation` tag records: the graphs are isomorphic once every node is
broken into 1 bp pieces.

The collapsed graphs are isomorphic if there is a bijection between their nodes that preserves the
sequences and the edges.
A node may also map to the reverse complement of another node.
Because flipping a node swaps its left and right sides, every edge endpoint at that node then
changes orientation.

Note that the reverse complement of a sequence preserves case here, and characters outside `ACGT`
map to themselves.
This differs from the usual convention, but it makes the reverse complement its own inverse, which
the canonical GFA format requires because it treats sequences as case sensitive.

A unitig can be traversed from either end, and the direction has to be chosen the same way in both
graphs.
The only information that survives chopping is the sequence, so the unitig is stored in the
direction where the sequence is lexicographically smaller.

### Command line

```txt
pggname --compare graph1 graph2
```

The verdict is written to standard output as `isomorphic`, `not isomorphic`, or `unresolved`,
followed by the two file names.
The reason for a negative answer is written to standard error.
The exit code is 0 for isomorphic, 1 for not isomorphic, and 2 for unresolved.

The `--integer-ids` and `--string-ids` options only choose how a GFA graph is stored in memory.
They cannot change the answer, because isomorphism does not depend on the node identifiers.
They may still determine whether the graph can be parsed at all.

### Translation

Use `--translation FILE` to write the correspondence between the two graphs.
A positive answer is a translation rather than a bijection between nodes, because the two graphs cut
the maximal non-branching paths in different places.

Each line has two walks separated by a tab: a maximal non-branching path in the first graph and the
path in the second graph that spells the same sequence.
A walk is a sequence of node names, each preceded by `>` for the forward orientation and `<` for the
reverse, as in a GFA W-line.

Here `trimmed.gfa` is `translation.gfa` without the segment that is not on any path, and
`translation.gbz` is the same graph chopped to at most 2 bp.

```txt
$ pggname --compare --translation translation.tsv trimmed.gfa translation.gbz
isomorphic      trimmed.gfa  translation.gbz
$ cat translation.tsv
>s11	>1>2
>s12	>3
>s13	>4
>s14	>5>6
>s15	>9
>s16	>10
>s17	>11
```

The 3 bp node `s11` of the first graph covers the 2 bp node `1` and the 1 bp node `2` of the second.

Each line is oriented so that the first node of the first walk is in forward orientation, when the
walk allows it.
A walk that begins in reverse and ends in forward orientation begins in reverse from either end; it
is left in the canonical direction of the path, where the sequence is lexicographically smaller.
For example, `>1>2` and `<5<4` on the same line mean that nodes 1 and 2 of the first graph, read in
the forward orientation, spell the same sequence as nodes 5 and 4 of the second graph, read in the
reverse orientation.

### Limitations

Graph isomorphism is not known to be solvable in polynomial time, and the implementation gives up
after a bounded search.
An `unresolved` answer is therefore a real possibility, though a rare one: the sequences make most
nodes easy to tell apart, and a realistic pangenome graph is usually settled without any search.

A positive answer is always verified against the sequences and the edges, so it is never wrong.
A negative answer is given only when it follows from an exact invariant or from an exhaustive
search.

A connected component that is a cycle with no branches has no unitig end to start from.
A canonical starting point would have to be defined by the minimal rotation of a circular sequence,
which may fall in the middle of a node.
Such components are reported as an error rather than handled incorrectly.

## Notes

* The included `.cargo/config.toml` sets the target CPU to `native`.
