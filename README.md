# Stable names for pangenome graphs

This is a prototype for generating stable names for pangenome graphs.
The names are based on hashing a canonical GFA representation of the graph.

See [refget](https://ga4gh.github.io/refget/) for a similar naming scheme for sequences.

## Intended applications

* Tagging various indexes with the name of the corresponding graph.
* As a reference name in a read alignment file.
* For representing relationships such as "A is a subgraph of B" or "A can be translated to B".
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

For each node, in sorted order, output:

* S-line for the node without optional fields.
* L-lines for all canonical edges, without the overlap field or optional fields, in sorted order.

The canonical GFA representation of the graph does not include any other information, such as header lines, paths, or walks.

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

## Canonical version

* Interpret node identifiers as integers if possible; fall back to string identifiers if not.
    * This allows using the natural order as the canonical order in common cases.
* Use SHA-256 as the hash.
    * SHA-512/256 would be faster, but it is not readily available on the command line.

## Other versions

* Graphs in GBZ and GFA formats.
* Node identifiers interpreted as integers or strings.
    * The canonical order of the nodes depends on the type of the identifiers.
    * Using string identifiers requires more memory.
    * String identifiers are faster with GFA graphs and slower with GBZ graphs.
* All SHA-2 variants.
