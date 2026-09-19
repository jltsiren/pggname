use gbz::{GBZ, GraphName};

use getopts::Options;

use pggname::{Graph, Topology};
use pggname::algorithms;
use pggname::comparison::{self, Verdict};
use pggname::graph::{GraphInt, GraphStr, GBZInt};
use pggname::isomorphism;
use pggname::topology::{GbzTopology, IndexedGraph};

use simple_sds::serialize;

use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, BufWriter, Write};
use std::{env, process};

//-----------------------------------------------------------------------------

// Exit code for an error. The other codes come from `Verdict::exit_code`.
const EXIT_ERROR: i32 = 5;

fn main() {
    let code = match run() {
        Ok(code) => code,
        Err(message) => {
            eprintln!("{}", message);
            EXIT_ERROR
        },
    };
    process::exit(code);
}

// FIXME: This should always print the names of the graphs.
// FIXME: If told to recompute, all existing name information should be discarded.
fn run() -> Result<i32, String> {
    let config = Config::new()?;
    if config.compare {
        compare_mode(&config)
    } else {
        hash_mode(&config).map(|_| 0)
    }
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
            "Usage: {} [options] graph1 [graph2 ...]\n       {} -c [options] graph1 graph2",
            program, program
        );

        let mut opts = Options::new();
        opts.optflag("s", "store-name", "store the name and the relationships in GBZ tags");
        opts.optflag("r", "recompute", "recompute the name even if it is stored in GBZ tags");
        opts.optflag("c", "compare", "determine the relationship between two graphs");
        opts.optopt("t", "translation", "write the translation to FILE (with -c)", "FILE");
        let matches = opts.parse(&args[1..]).map_err(|e| e.to_string())?;

        let input_files = if !matches.free.is_empty() {
            matches.free.clone()
        } else {
            eprintln!("{}", opts.usage(&header));
            process::exit(EXIT_ERROR);
        };
        let store_name = matches.opt_present("s");
        let recompute = matches.opt_present("r");
        let compare = matches.opt_present("c");
        let translation_file = matches.opt_str("t");

        if compare {
            if input_files.len() != 2 {
                return Err(format!("Option -c requires two graphs, but {} were given", input_files.len()));
            }
        } else if translation_file.is_some() {
            return Err(String::from("Option -t can only be used with -c"));
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

//-----------------------------------------------------------------------------

// Returns the name information for the GBZ graph, along with the graph itself and the information
// stored in the file.
//
// The name is recomputed if the file does not store one or if the user asked for it.
fn gbz_name(
    graph: GBZ, input_file: &str, config: &Config
) -> Result<(GBZ, GraphName, GraphName), String> {
    let stored = GraphName::from_tags(graph.tags())
        .map_err(|e| format!("Error parsing the name tags in {}: {}", input_file, e))?;
    if stored.has_name() && !config.recompute {
        return Ok((graph, stored.clone(), stored));
    }

    // `GBZInt` owns the graph, so we move the graph in and take it back afterwards.
    let wrapper = GBZInt { graph };
    let hash = pggname::stable_name(&wrapper);
    let graph = wrapper.graph;

    let mut name = GraphName::new(hash);
    if stored.has_name() && stored.name() != name.name() {
        // The relationships are keyed by the name of the graph, so a graph that is not the one the
        // tags describe cannot inherit them.
        eprintln!(
            "Warning: {} stores the name {}, but the graph is {}; discarding the stored relationships",
            input_file, stored.name().unwrap(), name.name().unwrap()
        );
    } else {
        name.add_relationships(&stored);
    }

    Ok((graph, name, stored))
}

// Returns the stable name of the GFA graph.
fn gfa_name(input_file: &str) -> Result<String, String> {
    // TODO: The header lines may store the name; see `algorithms::parse_gfa`.
    match read_gfa::<GraphInt>(input_file) {
        Ok(graph) => Ok(pggname::stable_name(&graph)),
        Err(_) => Ok(pggname::stable_name(&read_gfa::<GraphStr>(input_file)?)),
    }
}

// Returns a topological view of the GFA graph and its stable name.
//
// The parsed graph is dropped, as the view has all the information the comparison needs.
fn gfa_input(input_file: &str) -> Result<(IndexedGraph, String), String> {
    // TODO: The header lines may store the name; see `algorithms::parse_gfa`.
    match read_gfa::<GraphInt>(input_file) {
        Ok(graph) => Ok((IndexedGraph::from(&graph), pggname::stable_name(&graph))),
        Err(_) => {
            let graph = read_gfa::<GraphStr>(input_file)?;
            Ok((IndexedGraph::from(&graph), pggname::stable_name(&graph)))
        },
    }
}

//-----------------------------------------------------------------------------

fn hash_mode(config: &Config) -> Result<(), String> {
    for input_file in config.input_files.iter() {
        if GBZ::is_gbz(input_file) {
            let (graph, version) = read_gbz(input_file)?;
            let (mut graph, name, stored) = gbz_name(graph, input_file, config)?;
            println!("{}  {}", name.name().unwrap(), input_file);
            // Writing only on a change preserves the relationship tags, which `set_tags` would
            // otherwise clear, and avoids serializing a large graph for nothing.
            if config.store_name && name != stored {
                name.set_tags(graph.tags_mut());
                serialize::serialize_version_to(&graph, input_file, version)
                    .map_err(|e| format!("Error saving GBZ file {}: {}", input_file, e))?;
            }
        } else {
            let hash = gfa_name(input_file)?;
            println!("{}  {}", hash, input_file);
            if config.store_name {
                eprintln!("Warning: cannot store the name in GFA file {}", input_file);
            }
        }
    }

    Ok(())
}

//-----------------------------------------------------------------------------

// A graph that has been read from a file.
//
// GBZ graphs are kept as they are, because the topological view borrows the adjacency information
// instead of copying it. GFA graphs have to be indexed, as they store each edge at one endpoint
// only.
//
// The variants differ a lot in size, but there are exactly two of these per run.
#[allow(clippy::large_enum_variant)]
enum Source {
    Gbz { graph: GBZ, version: usize },
    Gfa(IndexedGraph),
}

// An input graph with its name information.
struct Input {
    filename: String,
    source: Source,
    // The name and the relationships, updated as the comparison finds new ones.
    name: GraphName,
    // The name and the relationships as they were in the file.
    stored: GraphName,
}

fn load_input(input_file: &str, config: &Config) -> Result<Input, String> {
    let filename = String::from(input_file);
    if GBZ::is_gbz(input_file) {
        let (graph, version) = read_gbz(input_file)?;
        let (graph, name, stored) = gbz_name(graph, input_file, config)?;
        Ok(Input { filename, source: Source::Gbz { graph, version }, name, stored })
    } else {
        let (graph, hash) = gfa_input(input_file)?;
        Ok(Input {
            filename,
            source: Source::Gfa(graph),
            name: GraphName::new(hash),
            stored: GraphName::default(),
        })
    }
}

// Writes the name information to the file, if it has changed.
//
// Returns `true` if the file was written.
fn store_name(input: &mut Input) -> Result<bool, String> {
    if input.name == input.stored {
        return Ok(false);
    }
    match &mut input.source {
        Source::Gbz { graph, version } => {
            input.name.set_tags(graph.tags_mut());
            serialize::serialize_version_to(graph, &input.filename, *version)
                .map_err(|e| format!("Error saving GBZ file {}: {}", input.filename, e))?;
            Ok(true)
        },
        Source::Gfa(_) => {
            eprintln!("Warning: cannot store the name in GFA file {}", input.filename);
            Ok(false)
        },
    }
}

// Returns `true` if the two paths refer to the same file.
fn same_file(first: &str, second: &str) -> bool {
    match (fs::canonicalize(first), fs::canonicalize(second)) {
        (Ok(first), Ok(second)) => first == second,
        _ => false,
    }
}

//-----------------------------------------------------------------------------

// Returns the exit code; see `Verdict::exit_code`.
fn compare_mode(config: &Config) -> Result<i32, String> {
    let mut first = load_input(&config.input_files[0], config)?;
    let mut second = load_input(&config.input_files[1], config)?;
    let verdict = compare_inputs(&first, &second, config)?;

    // The verdict goes to stdout, and everything else to stderr.
    println!("{:<14}  {}  {}", verdict.to_string(), first.filename, second.filename);
    if let Verdict::NotIsomorphic(mismatch) = verdict {
        eprintln!("The compacted graphs have {}.", mismatch);
    }
    if verdict == Verdict::Same && !first.name.is_same(&second.name) {
        eprintln!(
            "Warning: the graphs are the same, but they are named {} and {}",
            first.name.name().unwrap(), second.name.name().unwrap()
        );
    }

    // The verdict has already been reported, so a failure here does not hide it.
    if config.store_name {
        comparison::update_relationships(verdict, &mut first.name, &mut second.name);
        let both = !same_file(&first.filename, &second.filename);
        let mut stored = store_name(&mut first)?;
        // If the same file was given twice, the second write would be redundant.
        if both {
            stored |= store_name(&mut second)?;
        }
        if !stored {
            eprintln!("Note: there was no new name information to store");
        }
    }

    Ok(verdict.exit_code())
}

fn compare_inputs(first: &Input, second: &Input, config: &Config) -> Result<Verdict, String> {
    // The node identifiers are the same in both representations, so the two only differ in how the
    // graph is stored. The topological views live in this call, which keeps the borrows of the GBZ
    // graphs short enough for the name information to be updated afterwards.
    match (&first.source, &second.source) {
        (Source::Gbz { graph: a, .. }, Source::Gbz { graph: b, .. }) => {
            compare(&GbzTopology::new(a)?, &GbzTopology::new(b)?, &first.name, &second.name, config)
        },
        (Source::Gbz { graph: a, .. }, Source::Gfa(b)) => {
            compare(&GbzTopology::new(a)?, b, &first.name, &second.name, config)
        },
        (Source::Gfa(a), Source::Gbz { graph: b, .. }) => {
            compare(a, &GbzTopology::new(b)?, &first.name, &second.name, config)
        },
        (Source::Gfa(a), Source::Gfa(b)) => {
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
                let mut writer = create_translation_file(filename)?;
                isomorphism::write_translation(first, second, &translation, &mut writer)
                    .and_then(|()| writer.flush())
                    .map_err(|e| format!("Error writing translation file {}: {}", filename, e))?;
            },
            // Only an isomorphism has a translation; the other verdicts are settled without one.
            None => eprintln!("Note: there was no translation to write for verdict {}", verdict),
        }
    }

    Ok(verdict)
}

fn create_translation_file(filename: &str) -> Result<BufWriter<File>, String> {
    let file = File::create(filename)
        .map_err(|e| format!("Error creating translation file {}: {}", filename, e))?;
    Ok(BufWriter::new(file))
}

//-----------------------------------------------------------------------------
