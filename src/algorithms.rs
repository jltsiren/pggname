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
