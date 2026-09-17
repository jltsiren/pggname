use gbz::{GBZ, GraphName};

use getopts::Options;

use pggname::{Graph, Topology};
use pggname::graph::{GraphInt, GraphStr, GBZInt, GBZStr};
use pggname::algorithms;
use pggname::isomorphism::{self, UnitigIsomorphism};
use pggname::topology::{GbzTopology, IndexedGraph};

use sha2::{Digest, Sha224, Sha256, Sha384, Sha512_224, Sha512_256, Sha512};
use sha2::digest;

use simple_sds::serialize;

use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter};
use std::time::Instant;
use std::{env, process};

//-----------------------------------------------------------------------------

fn main() -> Result<(), String> {
    let config = Config::new()?;
    if config.compare {
        match compare_mode(&config) {
            Ok(code) => process::exit(code),
            Err(message) => {
                eprintln!("{}", message);
                process::exit(3);
            },
        }
    }
    hash_mode(&config)
}

//-----------------------------------------------------------------------------

fn hash_mode(config: &Config) -> Result<(), String> {
    for input_file in config.input_files.iter() {
        if GBZ::is_gbz(input_file) {
            let (graph, version) = read_gbz(input_file, config.benchmark)?;
            if config.node_ids == NodeIds::Integer || config.node_ids == NodeIds::Auto {
                let graph = GBZInt { graph };
                let hash = process(&graph, input_file, config.benchmark);
                if config.store_name && let Some(hash) = hash {
                    let mut graph = graph;
                    let tags = graph.graph.tags_mut();
                    let graph_name = GraphName::new(hash);
                    graph_name.set_tags(tags);
                    serialize::serialize_version_to(&graph.graph, input_file, version)
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
                    if let Ok(graph) = graph {
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
    compare: bool,
    translation_file: Option<String>,
}

impl Config {
    fn new() -> Result<Self, String> {
        let args: Vec<String> = env::args().collect();
        let program = args[0].clone();
        let header = format!(
            "Usage: {} [options] graph1 [graph2 ...]\n       {} -c [options] graph1 graph2",
            program, program
        );

        let mut opts = Options::new();
        opts.optflag("i", "integer-ids", "use integer node identifiers");
        opts.optflag("s", "string-ids", "use string node identifiers");
        opts.optflag("n", "store-name", "overwrite names and relationships in GBZ tags (not with -s, -b)");
        opts.optflag("b", "benchmark", "run benchmarks");
        opts.optflag("c", "compare", "determine whether two graphs are isomorphic (not with -n)");
        opts.optopt("t", "translation", "write the translation between the graphs to FILE (with -c)", "FILE");
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
        let compare = matches.opt_present("c");
        let translation_file = matches.opt_str("t");

        if compare {
            // Storing a name is a property of a single graph.
            if store_name {
                return Err(String::from("Option -n cannot be used with -c"));
            }
            if input_files.len() != 2 {
                return Err(format!("Option -c requires two graphs, but {} were given", input_files.len()));
            }
        } else if translation_file.is_some() {
            return Err(String::from("Option -t can only be used with -c"));
        }

        Ok(Config {
            input_files, node_ids, store_name, benchmark, compare, translation_file,
        })
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

    let graph = algorithms::parse_gfa::<G, _>(reader)?;

    let duration = start_time.elapsed();
    let seconds = duration.as_secs_f64();
    if benchmark {
        eprintln!("Parsed the graph in {:.3} seconds", seconds);
        eprintln!();
    }

    Ok(graph)
}

// Returns the graph itself and the version of the file format.
fn read_gbz(input_file: &str, benchmark: bool) -> Result<(GBZ, usize), String> {
    let start_time = Instant::now();

    let version = serialize::determine_version_from::<GBZ, _>(input_file)
        .map_err(|e| format!("Error determining GBZ version from {}: {}", input_file, e))?;
    let graph: GBZ = serialize::load_from(input_file)
        .map_err(|e| format!("Error loading GBZ file {}: {}", input_file, e))?;

    let duration = start_time.elapsed();
    let seconds = duration.as_secs_f64();
    if benchmark {
        eprintln!("Loaded the GBZ graph in {:.3} seconds", seconds);
        eprintln!();
    }

    Ok((graph, version))
}

//-----------------------------------------------------------------------------

fn process<G: Graph>(graph: &G, input_file: &str, benchmark: bool) -> Option<String> {
    if benchmark {
        print_statistics(graph, input_file);
        benchmark_all::<G>(graph);
        None
    } else {
        let hash = pggname::stable_name(graph);
        println!("{}  {}", hash, input_file);
        Some(hash)
    }
}

fn benchmark<D: Digest, G: Graph>(graph: &G, name: &str) 
    where digest::Output<D>: core::fmt::LowerHex {
    let start = Instant::now();
    let hash = algorithms::hash::<D, G>(graph);
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

//-----------------------------------------------------------------------------

// A graph that can provide a topological view of itself.
//
// GBZ graphs are kept as they are, because the view borrows the adjacency information instead of
// copying it. GFA graphs have to be indexed, as they store each edge at one endpoint only.
//
// The variants differ a lot in size, but there are exactly two of these per run.
#[allow(clippy::large_enum_variant)]
enum Input {
    Gbz(GBZ),
    Gfa(IndexedGraph),
}

fn read_input(input_file: &str, config: &Config) -> Result<Input, String> {
    if GBZ::is_gbz(input_file) {
        let (graph, _) = read_gbz(input_file, config.benchmark)?;
        return Ok(Input::Gbz(graph));
    }

    let start = Instant::now();
    let graph = match config.node_ids {
        NodeIds::Integer => IndexedGraph::from(&read_gfa::<GraphInt>(input_file, config.benchmark)?),
        NodeIds::String => IndexedGraph::from(&read_gfa::<GraphStr>(input_file, config.benchmark)?),
        NodeIds::Auto => match read_gfa::<GraphInt>(input_file, config.benchmark) {
            Ok(graph) => IndexedGraph::from(&graph),
            Err(_) => IndexedGraph::from(&read_gfa::<GraphStr>(input_file, config.benchmark)?),
        },
    };
    if config.benchmark {
        eprintln!("Indexed the graph in {:.3} seconds", start.elapsed().as_secs_f64());
        eprintln!();
    }

    Ok(Input::Gfa(graph))
}

// Returns the exit code: 0 for isomorphic, 1 for not isomorphic, and 2 for unresolved.
fn compare_mode(config: &Config) -> Result<i32, String> {
    let first = read_input(&config.input_files[0], config)?;
    let second = read_input(&config.input_files[1], config)?;

    // The node identifiers play no part in isomorphism, so the two representations only differ in
    // how the graph is stored.
    match (&first, &second) {
        (Input::Gbz(first), Input::Gbz(second)) => {
            compare(&GbzTopology::new(first)?, &GbzTopology::new(second)?, config)
        },
        (Input::Gbz(first), Input::Gfa(second)) => {
            compare(&GbzTopology::new(first)?, second, config)
        },
        (Input::Gfa(first), Input::Gbz(second)) => {
            compare(first, &GbzTopology::new(second)?, config)
        },
        (Input::Gfa(first), Input::Gfa(second)) => compare(first, second, config),
    }
}

fn compare<A: Topology, B: Topology>(first: &A, second: &B, config: &Config) -> Result<i32, String> {
    if config.benchmark {
        print_topology_statistics(first, &config.input_files[0]);
        print_topology_statistics(second, &config.input_files[1]);
    }

    let options = isomorphism::Options::default();
    let start = Instant::now();

    let (result, statistics) = isomorphism::are_isomorphic_unitigs_with_statistics(
        first, second, &options
    )?;
    report_timing(start, &statistics, config);
    if let (Some(filename), Some(translation)) = (&config.translation_file, result.translation()) {
        let mut writer = create_translation_file(filename)?;
        isomorphism::write_translation(first, second, translation, &mut writer)
            .map_err(|e| format!("Error writing translation file {}: {}", filename, e))?;
    }
    let reason = match &result {
        UnitigIsomorphism::NotIsomorphic(mismatch) => Some(format!(
            "The compacted graphs have {}.", mismatch
        )),
        _ => None,
    };
    let code = match result {
        UnitigIsomorphism::Isomorphic(_) => 0,
        UnitigIsomorphism::NotIsomorphic(_) => 1,
        UnitigIsomorphism::Unresolved => 2,
    };
    let verdict = result.to_string();

    // The verdict goes to stdout, and everything else to stderr.
    println!("{:<14}  {}  {}", verdict, config.input_files[0], config.input_files[1]);
    if let Some(reason) = reason {
        eprintln!("{}", reason);
    }

    Ok(code)
}

fn create_translation_file(filename: &str) -> Result<BufWriter<File>, String> {
    let file = File::create(filename)
        .map_err(|e| format!("Error creating translation file {}: {}", filename, e))?;
    Ok(BufWriter::new(file))
}

fn report_timing(start: Instant, statistics: &isomorphism::Statistics, config: &Config) {
    if !config.benchmark {
        return;
    }
    eprintln!("Compared the graphs in {:.3} seconds", start.elapsed().as_secs_f64());
    eprintln!("  Refinement rounds: {}", statistics.rounds);
    eprintln!("  Color classes:     {}", statistics.color_classes);
    eprintln!("  Choices:           {}", statistics.individualizations);
    eprintln!();
}

fn print_topology_statistics<T: Topology>(graph: &T, input_file: &str) {
    let (node_count, edge_count, seq_len) = graph.statistics();
    eprintln!("Graph {}:", input_file);
    eprintln!("  Nodes:    {}", node_count);
    eprintln!("  Edges:    {}", edge_count);
    eprintln!("  Sequence: {} bp", seq_len);
    eprintln!();
}

//-----------------------------------------------------------------------------
