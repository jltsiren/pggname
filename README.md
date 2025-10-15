# Stable names for pangenome graphs

This is a prototype for generating stable names for pangenome graphs.
The names are based on hashing a canonical GFA representation of the graph.

See [refget](https://ga4gh.github.io/refget/) for a similar naming scheme for sequences.

## Implemented versions

* Graphs in GBZ and GFA formats.
* Node identifiers interpreted as integers or strings.
    * The canonical order of the nodes depends on the type of the identifiers.
    * Using string identifiers requires more memory.
    * String identifiers are faster with GFA graphs and slower with GBZ graphs.
* All SHA-2 variants.

## Thoughts about a canonical version

* Interpret node identifiers as integers if possible; fall back to string identifiers if not.
    * This allows using the natural order as the canonical order in common cases.
* Use a variant of SHA-512 as the hash.
    * SHA-512 is faster than SHA-256 on relevant hardware.
    * Truncate the hashes to a reasonable length.

## Intended applications

* Tagging various indexes with the name of the corresponding graph.
* As a reference name in a read alignment file.
* For representing relationships such as "A is a subgraph of B" or "A can be translated to B".
    * If A is a subgraph of B, graph B can be used as a reference with reads aligned to A.
    * Some tools chop long nodes to smaller fragments, but coordinates in the chopped graph can be translated to the original coordinates.

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
