use gbz::{GBZ, GraphName};

use getopts::Options;

use pggname::{Graph, Topology};
use pggname::algorithms;
use pggname::comparison::{self, Verdict};
use pggname::graph::{GraphInt, GraphStr};
use pggname::isomorphism;
use pggname::topology::{GbzTopology, IndexedGraph};

use simple_sds::serialize;

use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Write};
use std::{env, process};

//-----------------------------------------------------------------------------

fn main() -> Result<(), String> {
    let config = Config::new()?;
    if config.compare {
        compare_mode(&config)
    } else {
        hash_mode(&config)
    }
}

fn compare_mode(config: &Config) -> Result<(), String> {
    // We could do this faster if the input files are the same, but naive comparison
    // should be good enough.
    let mut first = Input::load(&config.input_files[0], config)?;
    print_graph_name(&first.name, &first.filename);
    let mut second = Input::load(&config.input_files[1], config)?;
    print_graph_name(&second.name, &second.filename);
    let verdict = compare_inputs(&first, &second, config)?;

    // FIXME: what should we actually print
    // The verdict goes to stdout, and everything else to stderr.
    println!("{:<14}  {}  {}", verdict.to_string(), first.filename, second.filename);
    if let Verdict::NotIsomorphic(mismatch) = verdict {
        eprintln!("The compacted graphs have {}.", mismatch);
    }

    if config.store_name {
        comparison::update_relationships(verdict, &mut first.name, &mut second.name);
        first.store()?;
        second.store()?;
    }

    Ok(())
}

fn hash_mode(config: &Config) -> Result<(), String> {
    for input_file in config.input_files.iter() {
        if GBZ::is_gbz(input_file) {
            let (mut graph, version) = read_gbz(input_file)?;
            let (name, stored) = gbz_name(&graph, input_file, config)?;
            print_graph_name(&name, input_file);
            // Writing only on a change preserves the relationship tags, which `set_tags` would
            // otherwise clear, and avoids serializing a large graph for nothing.
            if config.store_name && name != stored {
                name.set_tags(graph.tags_mut());
                serialize::serialize_version_to(&graph, input_file, version)
                    .map_err(|e| format!("Error saving GBZ file {}: {}", input_file, e))?;
            }
        } else {
            let name = gfa_name(input_file)?;
            print_graph_name(&name, input_file);
        }
    }

    Ok(())
}

//-----------------------------------------------------------------------------

struct Config {
    input_files: Vec<String>,
    store_name: bool,
    recompute: bool,
    compare: bool,
    translation_file: Option<String>,
}

impl Config {
    fn new() -> Result<Self, String> {
        let args: Vec<String> = env::args().collect();
        let program = args[0].clone();
        let header = format!(
            "Usage: {} [options] graph1 [graph2 ...]\n       {} --compare [options] graph1 graph2",
            program, program
        );

        let mut opts = Options::new();
        opts.optflag("s", "store-name", "store the name and the relationships in GBZ tags");
        opts.optflag("r", "recompute", "discard existing name information and recompute");
        opts.optflag("c", "compare", "determine the relationship between two graphs");
        opts.optopt("t", "translation", "write the translation to FILE (with -c)", "FILE");
        let matches = opts.parse(&args[1..]).map_err(|e| e.to_string())?;

        let input_files = if !matches.free.is_empty() {
            matches.free.clone()
        } else {
            eprintln!("{}", opts.usage(&header));
            process::exit(1);
        };
        let store_name = matches.opt_present("s");
        let recompute = matches.opt_present("r");
        let compare = matches.opt_present("c");
        let translation_file = matches.opt_str("t");

        if compare {
            if input_files.len() != 2 {
                return Err(String::from("Option --compare requires two graphs"));
            }
        } else if translation_file.is_some() {
            return Err(String::from("Option --translation requires --compare"));
        }

        Ok(Config {
            input_files, store_name, recompute, compare, translation_file,
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

// Returns (graph name, stored name), computing the name as needed.
// If the name is (re)computed, all existing information will be discarded.
fn gbz_name(
    graph: &GBZ, input_file: &str, config: &Config
) -> Result<(GraphName, GraphName), String> {
    let stored = GraphName::from_tags(graph.tags())
        .map_err(|e| format!("Error parsing the name tags in {}: {}", input_file, e))?;
    if stored.has_name() && !config.recompute {
        return Ok((stored.clone(), stored));
    }
    let hash = pggname::stable_name(graph);
    let name = GraphName::new(hash);
    Ok((name, stored))
}

// FIXME: Parse stored name from GFA headers and include it in the output.
// Returns graph name, computing it as needed.
// If the name is (re)computed, all existing information will be discarded.
fn gfa_name(input_file: &str) -> Result<GraphName, String> {
    let hash = match read_gfa::<GraphInt>(input_file) {
        Ok(graph) => pggname::stable_name(&graph),
        Err(_) => pggname::stable_name(&read_gfa::<GraphStr>(input_file)?),
    };
    let name = GraphName::new(hash);
    Ok(name)
}

// FIXME: Parse stored name from GFA headers and include it in the output.
// Returns (indexex graph, graph name).
// If the name is (re)computed, all existing information will be discarded.
fn gfa_input(input_file: &str) -> Result<(IndexedGraph, GraphName), String> {
    match read_gfa::<GraphInt>(input_file) {
        Ok(graph) => {
            let hash = pggname::stable_name(&graph);
            let name = GraphName::new(hash);
            let graph = IndexedGraph::from(&graph);
            Ok((graph, name))
        },
        Err(_) => {
            let graph = read_gfa::<GraphStr>(input_file)?;
            let hash = pggname::stable_name(&graph);
            let name = GraphName::new(hash);
            let graph = IndexedGraph::from(&graph);
            Ok((graph, name))
        },
    }
}

// Prints graph name and file name to the standard output.
fn print_graph_name(name: &GraphName, input_file: &str) {
    println!("{}  {}", name.name().unwrap(), input_file);
}

//-----------------------------------------------------------------------------

// A graph that can be used as a source for `Topology`.
// GBZ graphs need a separate `GbzTopology`, while an `IndexedGraph` implements `Topology` directly.
// The variants differ a lot in size, but there are exactly two of these per run.
#[allow(clippy::large_enum_variant)]
enum TopologySource {
    Gbz { graph: GBZ, version: usize },
    Gfa(IndexedGraph),
}

// An input graph with its name information.
struct Input {
    filename: String,
    source: TopologySource,
    // Actual graph name.
    name: GraphName,
    // Graph name that was already stored in the graph.
    stored: GraphName,
}

impl Input {
    fn load(input_file: &str, config: &Config) -> Result<Self, String> {
        let filename = String::from(input_file);
        if GBZ::is_gbz(input_file) {
            let (graph, version) = read_gbz(input_file)?;
            let (name, stored) = gbz_name(&graph, input_file, config)?;
            Ok(Input { filename, source: TopologySource::Gbz { graph, version }, name, stored })
        } else {
            let (graph, name) = gfa_input(input_file)?;
            Ok(Input {
                filename,
                source: TopologySource::Gfa(graph),
                name,
                stored: GraphName::default(),
            })
        }
    }

    // Rewrites the graph with the updated name information, if it has changed and the graph format supports it.
    fn store(&mut self) -> Result<(), String> {
        if self.name == self.stored {
            return Ok(());
        }
        match &mut self.source {
            TopologySource::Gbz { graph, version } => {
                self.name.set_tags(graph.tags_mut());
                serialize::serialize_version_to(graph, &self.filename, *version)
                    .map_err(|e| format!("Error saving GBZ file {}: {}", self.filename, e))
            },
            TopologySource::Gfa(_) => Ok(()),
        }
    }
}

//-----------------------------------------------------------------------------

fn compare_inputs(first: &Input, second: &Input, config: &Config) -> Result<Verdict, String> {
    match (&first.source, &second.source) {
        (TopologySource::Gbz { graph: a, .. }, TopologySource::Gbz { graph: b, .. }) => {
            compare(&GbzTopology::new(a)?, &GbzTopology::new(b)?, &first.name, &second.name, config)
        },
        (TopologySource::Gbz { graph: a, .. }, TopologySource::Gfa(b)) => {
            compare(&GbzTopology::new(a)?, b, &first.name, &second.name, config)
        },
        (TopologySource::Gfa(a), TopologySource::Gbz { graph: b, .. }) => {
            compare(a, &GbzTopology::new(b)?, &first.name, &second.name, config)
        },
        (TopologySource::Gfa(a), TopologySource::Gfa(b)) => {
            compare(a, b, &first.name, &second.name, config)
        },
    }
}

fn compare<A: Topology, B: Topology>(
    first: &A, second: &B, first_name: &GraphName, second_name: &GraphName, config: &Config
) -> Result<Verdict, String> {
    let options = isomorphism::Options::default();
    let (verdict, translation) = comparison::compare(
        first, second, first_name, second_name, &options
    )?;

    if let Some(filename) = &config.translation_file {
        match translation {
            Some(translation) => {
                let file = File::create(filename)
                    .map_err(|e| format!("Error creating translation file {}: {}", filename, e))?;
                let mut writer = BufWriter::new(file);
                isomorphism::write_translation(first, second, &translation, &mut writer)
                    .and_then(|()| writer.flush())
                    .map_err(|e| format!("Error writing translation file {}: {}", filename, e))?;
            },
            None => (),
        }
    }

    Ok(verdict)
}

//-----------------------------------------------------------------------------
