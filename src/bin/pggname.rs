use gbz::{GBZ, GraphName};

use getopts::Options;

use pggname::{Graph, Topology};
use pggname::graph::{GraphInt, GraphStr, GBZInt};
use pggname::algorithms;
use pggname::isomorphism::{self, UnitigIsomorphism};
use pggname::topology::{GbzTopology, IndexedGraph};

use simple_sds::serialize;

use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter};
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

struct Config {
    input_files: Vec<String>,
    store_name: bool,
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
        opts.optflag("n", "store-name", "overwrite names and relationships in GBZ tags");
        opts.optflag("c", "compare", "determine whether two graphs are isomorphic (not with -n)");
        opts.optopt("t", "translation", "write the translation between the graphs to FILE (with -c)", "FILE");
        let matches = opts.parse(&args[1..]).map_err(|e| e.to_string())?;

        let input_files = if !matches.free.is_empty() {
            matches.free.clone()
        } else {
            eprintln!("{}", opts.usage(&header));
            process::exit(1);
        };
        let store_name = matches.opt_present("n");
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
            input_files, store_name, compare, translation_file,
        })
    }
}

//-----------------------------------------------------------------------------

// Parses the GFA graph, using integer node identifiers if possible and string identifiers
// otherwise.
fn read_gfa<G: Graph>(input_file: &str) -> Result<G, String> {
    let mut options = OpenOptions::new();
    let gfa_file = options.read(true).open(input_file)
        .map_err(|e| format!("Error opening GFA file {}: {}", input_file, e))?;
    let reader = BufReader::new(gfa_file);
    algorithms::parse_gfa::<G, _>(reader)
}

// Returns the graph itself and the version of the file format.
fn read_gbz(input_file: &str) -> Result<(GBZ, usize), String> {
    let version = serialize::determine_version_from::<GBZ, _>(input_file)
        .map_err(|e| format!("Error determining GBZ version from {}: {}", input_file, e))?;
    let graph: GBZ = serialize::load_from(input_file)
        .map_err(|e| format!("Error loading GBZ file {}: {}", input_file, e))?;
    Ok((graph, version))
}

//-----------------------------------------------------------------------------

fn hash_mode(config: &Config) -> Result<(), String> {
    for input_file in config.input_files.iter() {
        if GBZ::is_gbz(input_file) {
            let (graph, version) = read_gbz(input_file)?;
            let mut graph = GBZInt { graph };
            let hash = process(&graph, input_file);
            if config.store_name {
                let tags = graph.graph.tags_mut();
                let graph_name = GraphName::new(hash);
                graph_name.set_tags(tags);
                serialize::serialize_version_to(&graph.graph, input_file, version)
                    .map_err(|e| format!("Error saving GBZ file {}: {}", input_file, e))?;
            }
        } else if let Ok(graph) = read_gfa::<GraphInt>(input_file) {
            process(&graph, input_file);
        } else {
            let graph = read_gfa::<GraphStr>(input_file)?;
            process(&graph, input_file);
        }
    }

    Ok(())
}

fn process<G: Graph>(graph: &G, input_file: &str) -> String {
    let hash = pggname::stable_name(graph);
    println!("{}  {}", hash, input_file);
    hash
}

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

fn read_input(input_file: &str) -> Result<Input, String> {
    if GBZ::is_gbz(input_file) {
        let (graph, _) = read_gbz(input_file)?;
        return Ok(Input::Gbz(graph));
    }

    let graph = match read_gfa::<GraphInt>(input_file) {
        Ok(graph) => IndexedGraph::from(&graph),
        Err(_) => IndexedGraph::from(&read_gfa::<GraphStr>(input_file)?),
    };

    Ok(Input::Gfa(graph))
}

// Returns the exit code: 0 for isomorphic, 1 for not isomorphic, and 2 for unresolved.
fn compare_mode(config: &Config) -> Result<i32, String> {
    let first = read_input(&config.input_files[0])?;
    let second = read_input(&config.input_files[1])?;

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
    let options = isomorphism::Options::default();
    let result = isomorphism::are_isomorphic_unitigs(first, second, &options)?;

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

//-----------------------------------------------------------------------------
