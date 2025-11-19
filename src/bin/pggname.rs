use gbwt::GBZ;

use getopts::Options;

use pggname::{Graph, GraphStr, GraphInt, GBZStr, GBZInt};

use sha2::{Digest, Sha224, Sha256, Sha384, Sha512_224, Sha512_256, Sha512};
use sha2::digest;

use simple_sds::serialize;

use std::fs::OpenOptions;
use std::io::{BufRead, BufReader};
use std::time::Instant;
use std::{env, process};

//-----------------------------------------------------------------------------

fn main() -> Result<(), String> {
    let config = Config::new()?;

    for input_file in config.input_files.iter() {
        if GBZ::is_gbz(input_file) {
            let graph = read_gbz(input_file, config.benchmark)?;
            if config.node_ids == NodeIds::Integer || config.node_ids == NodeIds::Auto {
                let graph = GBZInt { graph };
                let hash = process(&graph, input_file, config.benchmark);
                if config.store_name && let Some(hash) = hash {
                    let mut graph = graph;
                    let tags = graph.graph.tags_mut();
                    // TODO: We should have a canonical source for the tag name.
                    tags.insert("pggname", &hash);
                    serialize::serialize_to(&graph.graph, input_file)
                        .map_err(|e| format!("Error saving GBZ file {}: {}", input_file, e))?;
                }
            } else {
                let graph = GBZStr { graph };
                process(&graph, input_file, config.benchmark);
            }
        } else {
            match config.node_ids {
                NodeIds::Integer => {
                    let graph = read_gfa::<GraphInt>(input_file, config.benchmark)?;
                    process(&graph, input_file, config.benchmark);
                }
                NodeIds::String => {
                    let graph = read_gfa::<GraphStr>(input_file, config.benchmark)?;
                    process(&graph, input_file, config.benchmark);
                }
                NodeIds::Auto => {
                    let graph = read_gfa::<GraphInt>(input_file, config.benchmark);
                    if graph.is_ok() {
                        let graph = graph.unwrap();
                        process(&graph, input_file, config.benchmark);
                    } else {
                        let graph = read_gfa::<GraphStr>(input_file, config.benchmark)?;
                        process(&graph, input_file, config.benchmark);
                    }
                }
            }
        }
    }

    Ok(())
}

//-----------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NodeIds {
    // Use integer identifiers if possible, fall back to string identifiers.
    Auto,
    // Use integer identifiers.
    Integer,
    // Use string identifiers.
    String,
}

struct Config {
    input_files: Vec<String>,
    node_ids: NodeIds,
    store_name: bool,
    benchmark: bool,
}

impl Config {
    fn new() -> Result<Self, String> {
        let args: Vec<String> = env::args().collect();
        let program = args[0].clone();
        let header = format!("Usage: {} [options] graph1 [graph2 ...]", &program);

        let mut opts = Options::new();
        opts.optflag("i", "integer-ids", "use integer node identifiers");
        opts.optflag("s", "string-ids", "use string node identifiers");
        opts.optflag("n", "store-name", "store the name in GBZ tags (not with -s, -b)");
        opts.optflag("b", "benchmark", "run benchmarks");
        let matches = opts.parse(&args[1..]).map_err(|e| e.to_string())?;

        let input_files = if !matches.free.is_empty() {
            matches.free.clone()
        } else {
            eprintln!("{}", opts.usage(&header));
            process::exit(1);
        };
        let node_ids = if matches.opt_present("i") {
            NodeIds::Integer
        } else if matches.opt_present("s") {
            NodeIds::String
        } else {
            NodeIds::Auto
        };
        let store_name = matches.opt_present("n");
        let benchmark = matches.opt_present("b");

        Ok(Config { input_files, node_ids, store_name, benchmark })
    }
}

//-----------------------------------------------------------------------------

fn print_statistics<G: Graph>(graph: &G, input_file: &str) {
    let (node_count, edge_count, seq_len) = graph.statistics();
    eprintln!("Graph {}:", input_file);
    eprintln!("  Nodes:    {}", node_count);
    eprintln!("  Edges:    {}", edge_count);
    eprintln!("  Sequence: {} bp", seq_len);
    eprintln!();
}

fn read_gfa<G: Graph>(input_file: &str, benchmark: bool) -> Result<G, String> {
    let start_time = Instant::now();

    // Open the input GFA file.
    let mut options = OpenOptions::new();
    let gfa_file = options.read(true).open(input_file)
        .map_err(|e| format!("Error opening GFA file {}: {}", input_file, e))?;
    let reader = BufReader::new(gfa_file);

    // Read and validate the graph.
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
            let source_o = pggname::parse_orientation(fields[2])
                .map_err(|e| format!("Error parsing GFA line {}: {}", i + 1, e))?;
            let dest_name = fields[3];
            let dest_o = pggname::parse_orientation(fields[4])
                .map_err(|e| format!("Error parsing GFA line {}: {}", i + 1, e))?;
            graph.add_edge(source_name, source_o, dest_name, dest_o)?;
        }
    }
    graph.finalize()?;

    let duration = start_time.elapsed();
    let seconds = duration.as_secs_f64();
    if benchmark {
        eprintln!("Parsed the graph in {:.3} seconds", seconds);
        eprintln!();
    }

    Ok(graph)
}

fn read_gbz(input_file: &str, benchmark: bool) -> Result<GBZ, String> {
    let start_time = Instant::now();

    let graph: GBZ = serialize::load_from(input_file)
        .map_err(|e| format!("Error loading GBZ file {}: {}", input_file, e))?;

    let duration = start_time.elapsed();
    let seconds = duration.as_secs_f64();
    if benchmark {
        eprintln!("Loaded the GBZ graph in {:.3} seconds", seconds);
        eprintln!();
    }

    Ok(graph)
}

//-----------------------------------------------------------------------------

fn process<G: Graph>(graph: &G, input_file: &str, benchmark: bool) -> Option<String> {
    if benchmark {
        print_statistics(graph, input_file);
        benchmark_all::<G>(graph);
        None
    } else {
        let hash = hash::<Sha256, G>(graph);
        println!("{}  {}", hash, input_file);
        Some(hash)
    }
}

fn hash<D: Digest, G: Graph>(graph: &G) -> String
    where digest::Output<D>: core::fmt::LowerHex {
    let mut hasher = D::new();
    for bytes in graph.node_iter() {
        hasher.update(&bytes);
    }
    let hash = hasher.finalize();
    format!("{:x}", hash)
}

fn benchmark<D: Digest, G: Graph>(graph: &G, name: &str) 
    where digest::Output<D>: core::fmt::LowerHex {
    let start = Instant::now();
    let hash = hash::<D, G>(graph);
    let duration = start.elapsed();
    let seconds = duration.as_secs_f64();
    eprintln!("{}: {}", name, hash);
    eprintln!("Used {:.3} seconds", seconds);
    eprintln!()
}

fn benchmark_all<G: Graph>(graph: &G) {
    benchmark::<Sha224, G>(graph, "SHA-224");
    benchmark::<Sha256, G>(graph, "SHA-256");
    benchmark::<Sha384, G>(graph, "SHA-384");
    benchmark::<Sha512_224, G>(graph, "SHA-512/224");
    benchmark::<Sha512_256, G>(graph, "SHA-512/256");
    benchmark::<Sha512, G>(graph, "SHA-512");
}

//-----------------------------------------------------------------------------
