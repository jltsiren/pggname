# Stable names for pangenome graphs

This is a proposal for generating stable names for pangenome graphs.
The names are SHA-256 hashes of a canonical GFA representation of the graph.

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

## Command line

### Basic usage

```sh
pggname [options] graph1 [graph2 ...]
```

The tool writes the name of the graph and the file name to standard output in the same manner as `sha*sum`.
If a GBZ graph already stores its name in the tags, that name is printed instead of computing it.
Name information is currently not read from GFA headers.
Use `--recompute` to discard stored name information and to recompute it.

Use `--store-name` to write the name and the relationships to the GBZ tags.
The file is rewritten only if the tags would change, and the relationships already stored in it are preserved.
Note that the file is rewritten in place, so a failure during the write destroys the graph.
This option does not work with GFA graphs, as the in-memory representation does not store enough information to reproduce the file.

### Comparing graphs

```sh
pggname --compare [options] graph1 graph2
```

Option `--compare` requires two graphs.
In addition to determining graph names, the tool will also try to determine the relationship between the graphs.
The tool will consider if the graphs are identical, if one of them is a subgraph of the other, or if the graphs are isomorphic.
Indirect relationships such as subgraph isomorphism will not be determined.
If option `--store-name` is used with GBZ graphs, the determined relationship will be stored along with graph names.

Graph isomorphism will be determined at unitig level, and a unitig may be matched to its reverse complent in the other graph.
If two graphs are otherwise the same, but one has had its nodes chopped to a maximum length, the graphs will be considered isomorphic.
While determining graph isomorphism can be computationally expensive, sequence labels make it simple with pangenome graphs.
Two human pangenome graphs can be compared in a matter of minutes.
In degenerate cases, the tool will leave the relationship unresolved rather than proceed with expensive computations.

If option `--translation FILE` is given, the translation between the unitigs of isomorphic graphs will be written to `FILE`.
The output will contain one line per unitig, listing the matched walks as TAB-separated fields.
Example output for `trimmed.gfa` and `translation.gbz` in `test-data/`:

```txt
>s11	>1>2
>s12	>3
>s13	>4
>s14	>5>6
>s15	>9
>s16	>10
>s17	>11
```

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

## Notes

* The included `.cargo/config.toml` sets the target CPU to `native`.
