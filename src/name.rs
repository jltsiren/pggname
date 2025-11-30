//! A structure storing a graph name and relationships between graphs.

// FIXME: document

use gbwt::support::Tags;

use std::collections::{BTreeMap, BTreeSet};

//-----------------------------------------------------------------------------

// FIXME: document, example, tests
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GraphName {
    name: Option<String>,
    subgraph: BTreeMap<String, BTreeSet<String>>,
    translation: BTreeMap<String, BTreeSet<String>>,
}

// FIXME: implement
// * is subgraph of / translates to
// * describe relationship

impl GraphName {
    /// Name of the tag storing the graph name.
    const TAG_NAME: &'static str = "pggname";

    /// Name of the tag storing subgraph relationships.
    const TAG_SUBGRAPH: &'static str = "subgraph";

    /// Name of the tag storing translation relationships.
    const TAG_TRANSLATION: &'static str = "translation";

    /// GFA header tag storing the graph name.
    const GFA_HEADER_NAME: &'static str = "NM";

    /// GAF header tag storing the graph name.
    const GAF_HEADER_NAME: &'static str = "RN";

    /// GFA/GAF header tag storing subgraph relationships.
    const GFA_GAF_HEADER_SUBGRAPH: &'static str = "SG";

    /// GFA/GAF header tag storing translation relationships.
    const GFA_GAF_HEADER_TRANSLATION: &'static str = "TL";

    const GFA_HEADER_TYPE: &'static str = "H";
    const GAF_HEADER_PREFIX: &'static str = "@"; 
    const GFA_GAF_FIELD_SEPARATOR: char = '\t';
    const TAG_GFA_RELATIONSHIP_SEPARATOR: char = ',';
    const TAG_RELATIONSHIP_LIST_SEPARATOR: char = ';';

    /// Creates a new `GraphName` with the given stable graph name.
    pub fn new(name: String) -> Self {
        GraphName {
            name: Some(name),
            subgraph: BTreeMap::new(),
            translation: BTreeMap::new(),
        }
    }

    /// Parses a `GraphName` from the given tags.
    ///
    /// Returns an error if tag values are malformed.
    pub fn from_tags(tags: &Tags) -> Result<Self, String> {
        let mut result = GraphName::default();

        if let Some(name_field) = tags.get(Self::TAG_NAME) {
            result.name = Some(String::from(name_field));
        }

        if let Some(subgraph_field) = tags.get(Self::TAG_SUBGRAPH) {
            let relationships: Vec<&str> = subgraph_field.split(Self::TAG_RELATIONSHIP_LIST_SEPARATOR).collect();
            for rel in relationships {
                let parts: Vec<&str> = rel.split(Self::TAG_GFA_RELATIONSHIP_SEPARATOR).collect();
                if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
                    return Err(format!("Invalid subgraph relationship: {}", rel));
                }
                result.subgraph
                    .entry(String::from(parts[0]))
                    .or_default()
                    .insert(String::from(parts[1]));
            }
        }

        if let Some(translation_field) = tags.get(Self::TAG_TRANSLATION) {
            let relationships: Vec<&str> = translation_field.split(Self::TAG_RELATIONSHIP_LIST_SEPARATOR).collect();
            for rel in relationships {
                let parts: Vec<&str> = rel.split(Self::TAG_GFA_RELATIONSHIP_SEPARATOR).collect();
                if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
                    return Err(format!("Invalid translation relationship: {}", rel));
                }
                result.translation
                    .entry(String::from(parts[0]))
                    .or_default()
                    .insert(String::from(parts[1]));
            }
        }

        Ok(result)
    }

    fn typed_field_is_string(field: &str) -> Result<bool, String> {
        let bytes = field.as_bytes();
        if field.len() < 5 || bytes[2] != b':' || bytes[4] != b':' {
            return Err(format!("Invalid GFA typed field: {}", field));
        }
        Ok(bytes[3] == b'Z')
    }

    fn parse_gfa_optional_fields(fields: &[&str], result: &mut GraphName) -> Result<(), String> {
        for &field in fields {
            if !Self::typed_field_is_string(field)? {
                continue;
            }
            let tag = &field[0..2];
            let value = &field[5..];
            match tag {
                Self::GFA_HEADER_NAME => {
                    result.name = Some(String::from(value));
                }
                Self::GFA_GAF_HEADER_SUBGRAPH => {
                    let parts: Vec<&str> = value.split(Self::TAG_GFA_RELATIONSHIP_SEPARATOR).collect();
                    if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
                        return Err(format!("Invalid subgraph field: {}", field));
                    }
                    result.subgraph
                        .entry(String::from(parts[0]))
                        .or_default()
                        .insert(String::from(parts[1]));
                }
                Self::GFA_GAF_HEADER_TRANSLATION => {
                    let parts: Vec<&str> = value.split(Self::TAG_GFA_RELATIONSHIP_SEPARATOR).collect();
                    if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
                        return Err(format!("Invalid translation field: {}", field));
                    }
                    result.translation
                        .entry(String::from(parts[0]))
                        .or_default()
                        .insert(String::from(parts[1]));
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn parse_gaf_header_fields(line: &str, fields: &[&str], result: &mut GraphName) -> Result<(), String> {
        match &fields[0][1..] {
            Self::GAF_HEADER_NAME => {
                if fields.len() != 2 || fields[1].is_empty() {
                    return Err(format!("Invalid GAF name header line: {}", line));
                }
                result.name = Some(String::from(fields[1]));
            }
            Self::GFA_GAF_HEADER_SUBGRAPH => {
                if fields.len() != 3 || fields[1].is_empty() || fields[2].is_empty() {
                    return Err(format!("Invalid GAF subgraph header line: {}", line));
                }
                result.subgraph
                    .entry(String::from(fields[1]))
                    .or_default()
                    .insert(String::from(fields[2]));
            }
            Self::GFA_GAF_HEADER_TRANSLATION => {
                if fields.len() != 3 || fields[1].is_empty() || fields[2].is_empty() {
                    return Err(format!("Invalid GAF translation header line: {}", line));
                }
                result.translation
                    .entry(String::from(fields[1]))
                    .or_default()
                    .insert(String::from(fields[2]));
            }
            _ => {}
        }
        Ok(())
    }

    /// Parses a `GraphName` from the given GFA/GAF header lines.
    ///
    /// Returns an error if the lines cannot be parsed.
    pub fn from_header_lines(lines: &[String]) -> Result<Self, String> {
        let mut result = GraphName::default();

        for (i, line) in lines.iter().enumerate() {
            let fields: Vec<&str> = line.split(Self::GFA_GAF_FIELD_SEPARATOR).collect();
            if fields.len() < 2 {
                return Err(format!("Error parsing header line {}: not enough fields", i + 1));
            }
            if fields[0] == Self::GFA_HEADER_TYPE {
                Self::parse_gfa_optional_fields(&fields[1..], &mut result)?;
            } else if fields[0].len() == 3 && fields[0].starts_with(Self::GAF_HEADER_PREFIX) {
                Self::parse_gaf_header_fields(line, &fields, &mut result)?;
            } else {
                return Err(format!("Error parsing header line {}: unknown first field {}", i + 1, fields[0]));
            }
        }

        Ok(result)
    }

    /// Adds a new subgraph relationship, if both names are available.
    pub fn add_subgraph(&mut self, subgraph: &GraphName, supergraph: &GraphName) {
        if let (Some(subgraph_name), Some(supergraph_name)) = (subgraph.name(), supergraph.name()) {
            self.subgraph
                .entry(supergraph_name.clone())
                .or_default()
                .insert(subgraph_name.clone());
        }
    }

    /// Adds a new translation relationship, if both names are available.
    pub fn add_translation(&mut self, from: &GraphName, to: &GraphName) {
        if let (Some(from_name), Some(to_name)) = (from.name(), to.name()) {
            self.translation
                .entry(from_name.clone())
                .or_default()
                .insert(to_name.clone());
        }
    }

    /// Returns the name of the graph, if available.
    pub fn name(&self) -> Option<&String> {
        self.name.as_ref()
    }

    /// Returns `true` if both objects represent the same graph.
    pub fn is_same(&self, other: &GraphName) -> bool {
        match (&self.name, &other.name) {
            (Some(name1), Some(name2)) => name1 == name2,
            _ => false,
        }
    }

    /// Returns an iterator over stored subgraph relationships.
    ///
    /// The iterator yields pairs `(subgraph_name, supergraph_name)` in sorted order.
    pub fn subgraph_iter(&self) -> impl Iterator<Item = (&String, &String)> {
        self.subgraph.iter().flat_map(|(supergraph, subgraphs)| {
            subgraphs.iter().map(move |subgraph| (subgraph, supergraph))
        })
    }

    /// Returns an iterator over stored translation relationships.
    ///
    /// The iterator yields pairs `(from_name, to_name)` in sorted order.
    pub fn translation_iter(&self) -> impl Iterator<Item = (&String, &String)> {
        self.translation.iter().flat_map(|(from, tos)| {
            tos.iter().map(move |to| (from, to))
        })
    }

    // FIXME: write_tags

    /// Returns GFA header lines representing this object.
    pub fn to_gfa_header_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if let Some(name) = &self.name {
            lines.push(format!("H\t{}:Z:{}", Self::GFA_HEADER_NAME, name));
        }
        for (subgraph, supergraphs) in &self.subgraph {
            for supergraph in supergraphs {
                lines.push(format!("H\t{}:Z:{},{}", Self::GFA_GAF_HEADER_SUBGRAPH, subgraph, supergraph));
            }
        }
        for (from, tos) in &self.translation {
            for to in tos {
                lines.push(format!("H\t{}:Z:{},{}", Self::GFA_GAF_HEADER_TRANSLATION, from, to));
            }
        }
        lines
    }

    /// Returns GAF header lines representing this object.
    pub fn to_gaf_header_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if let Some(name) = &self.name {
            lines.push(format!("@{}\t{}", Self::GAF_HEADER_NAME, name));
        }
        for (subgraph, supergraphs) in &self.subgraph {
            for supergraph in supergraphs {
                lines.push(format!("@{}\t{}\t{}", Self::GFA_GAF_HEADER_SUBGRAPH, subgraph, supergraph));
            }
        }
        for (from, tos) in &self.translation {
            for to in tos {
                lines.push(format!("@{}\t{}\t{}", Self::GFA_GAF_HEADER_TRANSLATION, from, to));
            }
        }
        lines
    }
}

//-----------------------------------------------------------------------------
