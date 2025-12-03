//! Algorithms for computing stable graph names.

use crate::Graph;

use gbwt::Orientation;

use sha2::Digest;
use sha2::digest;

use std::io::BufRead;

//-----------------------------------------------------------------------------

/// Builds a graph from the given GFA input.
///
/// Returns an error if reading the input fails of if the GFA cannot be parsed.
/// Passes through errors from the graph methods.
///
/// # Examples
///
/// ```
/// use pggname::Graph;
/// use pggname::algorithms;
/// use pggname::graph::GraphInt;
/// use gbwt::support;
/// use std::fs::OpenOptions;
/// use std::io::BufReader;
///
/// let filename = support::get_test_data("example.gfa");
/// let file = OpenOptions::new().read(true).open(&filename).unwrap();
/// let reader = BufReader::new(file);
/// let graph = algorithms::parse_gfa::<GraphInt, _>(reader);
/// assert!(graph.is_ok());
///
/// let graph = graph.unwrap();
/// let (node_count, edge_count, seq_len) = graph.statistics();
/// assert_eq!(node_count, 12);
/// assert_eq!(edge_count, 13);
/// assert_eq!(seq_len, 12);
/// ```
pub fn parse_gfa<G: Graph, R: BufRead>(reader: R) -> Result<G, String> {
    let mut graph = G::new();
    for (i, line) in reader.split(b'\n').enumerate() {
        let line = line.map_err(|e| format!("Error reading GFA line {}: {}", i + 1, e))?;
        if line.is_empty() {
            continue;
        }
        if line[0] == b'S' {
            let fields: Vec<&[u8]> = line.split(|&c| c == b'\t').collect();
            if fields.len() < 3 {
                return Err(format!("Error parsing GFA line {}: not enough fields for a segment", i + 1));
            }
            graph.add_node(fields[1], fields[2])?;
        } else if line[0] == b'L' {
            let fields: Vec<&[u8]> = line.split(|&c| c == b'\t').collect();
            if fields.len() < 5 {
                return Err(format!("Error parsing GFA line {}: not enough fields for a link", i + 1));
            }
            let source_name = fields[1];
            let source_o = parse_orientation(fields[2])
                .map_err(|e| format!("Error parsing GFA line {}: {}", i + 1, e))?;
            let dest_name = fields[3];
            let dest_o = parse_orientation(fields[4])
                .map_err(|e| format!("Error parsing GFA line {}: {}", i + 1, e))?;
            graph.add_edge(source_name, source_o, dest_name, dest_o)?;
        }
    }
    graph.finalize()?;

    Ok(graph)
}

//-----------------------------------------------------------------------------

/// Computes the given hash of the canonical GFA representation of the given graph.
///
/// # Examples
///
/// ```
/// use pggname::Graph;
/// use pggname::algorithms;
/// use pggname::graph::GBZInt;
/// use gbwt::GBZ;
/// use gbwt::support;
/// use sha2::Sha256;
/// use simple_sds::serialize;
///
/// let filename = support::get_test_data("example.gbz");
/// let gbz: GBZ = serialize::load_from(&filename).unwrap();
/// let graph = GBZInt { graph: gbz };
/// let hash = algorithms::hash::<Sha256, _>(&graph);
/// assert_eq!(hash, "81b160c814182a12aaf95fd458e191590e95fb13c71e1c2f61ff827f605cf970");
/// ```
pub fn hash<D: Digest, G: Graph>(graph: &G) -> String
    where digest::Output<D>: core::fmt::LowerHex {
    let mut hasher = D::new();
    for bytes in graph.node_iter() {
        hasher.update(&bytes);
    }
    let hash = hasher.finalize();
    format!("{:x}", hash)
}

//-----------------------------------------------------------------------------

// Parses the orientation from GFA field.
fn parse_orientation(field: &[u8]) -> Result<Orientation, String> {
    match field {
        b"+" => Ok(Orientation::Forward),
        b"-" => Ok(Orientation::Reverse),
        _ => Err(format!("Invalid orientation: {}", String::from_utf8_lossy(field))),
    }
}

//-----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{GBZInt, GBZStr, GraphInt, GraphStr};

    use gbwt::GBZ;
    use gbwt::support;
    use sha2::Sha256;
    use simple_sds::serialize;

    use std::fs::OpenOptions;
    use std::io::BufReader;

    struct TestCase {
        gfa_name: &'static str,
        gbz_name: &'static str,
        hash_gfa_int: &'static str,
        hash_gbz_int: &'static str,
        hash_gfa_str: &'static str,
        hash_gbz_str: &'static str,
    }

    fn get_test_cases() -> Vec<TestCase> {
        vec![
            TestCase {
                gfa_name: "example.gfa",
                gbz_name: "example.gbz",
                hash_gfa_int: "81b160c814182a12aaf95fd458e191590e95fb13c71e1c2f61ff827f605cf970",
                hash_gbz_int: "81b160c814182a12aaf95fd458e191590e95fb13c71e1c2f61ff827f605cf970",
                hash_gfa_str: "81b160c814182a12aaf95fd458e191590e95fb13c71e1c2f61ff827f605cf970",
                hash_gbz_str: "81b160c814182a12aaf95fd458e191590e95fb13c71e1c2f61ff827f605cf970",
            },
            TestCase {
                gfa_name: "translation.gfa",
                gbz_name: "translation.gbz",
                hash_gfa_int: "", // Non-numeric node ids.
                hash_gbz_int: "b834b724b50976560291fe7dc25d57679d513078158cdbf61dfa5a575cbd0497",
                hash_gfa_str: "2e6552e5e455f0d24f75c40f14ea66c0c67600861014853d55be842ced5f2ba4",
                hash_gbz_str: "e55ba1e7aad84b6735b4ca4ca46d7d1986a02864174aa66fe75b363e7e2d31d6",
            }
        ]
    }

    #[test]
    fn test_gfa() {
        let test_cases = get_test_cases();
        for test_case in test_cases.iter() {
            let filename = support::get_test_data(&test_case.gfa_name);
            if !test_case.hash_gfa_int.is_empty() {
                let file = OpenOptions::new()
                    .read(true)
                    .open(&filename)
                    .unwrap();
                let reader = BufReader::new(file);
                let graph_int: GraphInt = parse_gfa(reader).unwrap();
                let hash_int = hash::<Sha256, _>(&graph_int);
                assert_eq!(&hash_int, test_case.hash_gfa_int, "Wrong hash for GraphInt {}", test_case.gfa_name);
            }

            let file = OpenOptions::new()
                .read(true)
                .open(&filename)
                .unwrap();
            let reader = BufReader::new(file);
            let graph_str: GraphStr = parse_gfa(reader).unwrap();
            let hash_str = hash::<Sha256, _>(&graph_str);
            assert_eq!(&hash_str, test_case.hash_gfa_str, "Wrong hash for GraphStr {}", test_case.gfa_name);
        }
    }

    #[test]
    fn test_gbz() {
        let test_cases = get_test_cases();
        for test_case in test_cases.iter() {
            let filename = support::get_test_data(&test_case.gbz_name);
            let gbz: GBZ = serialize::load_from(&filename).unwrap();

            let gbz_int = GBZInt { graph: gbz.clone() };
            let hash_int = hash::<Sha256, _>(&gbz_int);
            assert_eq!(&hash_int, test_case.hash_gbz_int, "Wrong hash for GBZInt {}", test_case.gbz_name);

            let gbz_str = GBZStr { graph: gbz.clone() };
            let hash_str = hash::<Sha256, _>(&gbz_str);
            assert_eq!(&hash_str, test_case.hash_gbz_str, "Wrong hash for GBZStr {}", test_case.gbz_name);
        }
    }
}

//-----------------------------------------------------------------------------
