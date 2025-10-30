use gbwt::GBZ;

use getopts::Options;

use pggname::{Graph, GraphStr, GraphInt, GBZStr, GBZInt};

use sha2::{Digest, Sha224, Sha256, Sha384, Sha512_224, Sha512_256, Sha512};
use sha2::digest;

use simple_sds::serialize;

use std::fs::OpenOptions;
use std::io::{BufRead, BufReader};
use std::time::Instant;
use std::env;

//-----------------------------------------------------------------------------

fn main() -> Result<(), String> {
    let config = Config::new()?;

    if config.gbz_input {
        let graph = read_gbz(&config)?;
        if config.integer_ids {
            let graph = GBZInt { graph };
            if config.benchmark {
                print_statistics(&graph);
                benchmark_all::<GBZInt>(&graph);
            } else {
                let hash = hash::<Sha256, GBZInt>(&graph);
                println!("{}", hash);
            }
        } else {
            let graph = GBZStr { graph };
            if config.benchmark {
                print_statistics(&graph);
                benchmark_all::<GBZStr>(&graph);
            } else {
                let hash = hash::<Sha256, GBZStr>(&graph);
                println!("{}", hash);
            }
        }
    } else {
        if config.integer_ids {
            let graph = read_gfa::<GraphInt>(&config)?;
            if config.benchmark {
                print_statistics(&graph);
                benchmark_all::<GraphInt>(&graph);
            } else {
                let hash = hash::<Sha256, GraphInt>(&graph);
                println!("{}", hash);
            }
        } else {
            let graph = read_gfa::<GraphStr>(&config)?;
            if config.benchmark {
                print_statistics(&graph);
                benchmark_all::<GraphStr>(&graph);
            } else {
                let hash = hash::<Sha256, GraphStr>(&graph);
                println!("{}", hash);
            }
        }
    }

    Ok(())
}

//-----------------------------------------------------------------------------

struct Config {
    input_file: String,
    gbz_input: bool,
    integer_ids: bool,
    benchmark: bool,
}

impl Config {
    fn new() -> Result<Self, String> {
        let args: Vec<String> = env::args().collect();
        let program = args[0].clone();
        let header = format!("Usage: {} [options] graph.[gbz|gfa]", &program);

        let mut opts = Options::new();
        opts.optflag("g", "gbz", "input is a GBZ graph (default: GFA)");
        opts.optflag("i", "integer-ids", "use integer node identifiers");
        opts.optflag("b", "benchmark", "run benchmarks");
        let matches = opts.parse(&args[1..]).map_err(|e| e.to_string())?;

        let input_file = if let Some(f) = matches.free.get(0) {
            f.clone()
        } else {
            return Err(opts.usage(&header));
        };
        let gbz_input = matches.opt_present("g");
        let integer_ids = matches.opt_present("i");
        let benchmark = matches.opt_present("b");

        Ok(Config { input_file, gbz_input, integer_ids, benchmark })
    }
}

//-----------------------------------------------------------------------------

fn print_statistics<G: Graph>(graph: &G) {
    let (node_count, edge_count, seq_len) = graph.statistics();
    eprintln!("Graph statistics:");
    eprintln!("  Nodes:    {}", node_count);
    eprintln!("  Edges:    {}", edge_count);
    eprintln!("  Sequence: {} bp", seq_len);
    eprintln!();
}

fn read_gfa<G: Graph>(config: &Config) -> Result<G, String> {
    let start_time = Instant::now();

    // Open the input GFA file.
    let mut options = OpenOptions::new();
    let gfa_file = options.read(true).open(&config.input_file)
        .map_err(|e| format!("Error opening GFA file {}: {}", config.input_file, e))?;
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
    if config.benchmark {
        eprintln!("Parsed the graph in {:.3} seconds", seconds);
        eprintln!();
        print_statistics(&graph);
    }

    Ok(graph)
}

fn read_gbz(config: &Config) -> Result<GBZ, String> {
    let start_time = Instant::now();

    let graph: GBZ = serialize::load_from(&config.input_file)
        .map_err(|e| format!("Error loading GBZ file {}: {}", config.input_file, e))?;

    let duration = start_time.elapsed();
    let seconds = duration.as_secs_f64();
    if config.benchmark {
        eprintln!("Loaded the GBZ graph in {:.3} seconds", seconds);
        eprintln!();
    }

    Ok(graph)
}

//-----------------------------------------------------------------------------

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
