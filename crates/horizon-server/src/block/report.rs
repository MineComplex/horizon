//! Reading the table from vanilla's data generator report `reports/blocks.json`.
//!
//! The report is Mojang's data, so it is never committed: `tools/leaf_server_reports.py` runs the
//! generator once on the server owner's own 26.3 server jar and writes the reports under the
//! server directory (ARCHITECTURE.md, "Data"). The report lists every block with its properties and
//! states (`{"properties": {name: [values]}, "states": [{"id", "properties", "default"}]}`); its
//! keys are reordered by the JSON writer (`DataProvider.KEY_COMPARATOR` puts `type` first), so the
//! loader sorts properties by name itself and checks every state id against the order it computes.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::{BlockDataError, BlockState, BlockStates, BlockStatesBuilder};

/// Where the prepare tool writes the report, relative to the server directory.
pub fn default_report_path(server_dir: &Path) -> PathBuf {
    server_dir.join("cache/vanilla-26.3/reports/blocks.json")
}

#[derive(Deserialize)]
struct ReportBlock {
    #[serde(default)]
    properties: BTreeMap<String, Vec<String>>,
    states: Vec<ReportState>,
}

#[derive(Deserialize)]
struct ReportState {
    id: u32,
    #[serde(default)]
    default: bool,
    #[serde(default)]
    properties: HashMap<String, String>,
}

impl BlockStates {
    /// Reads `blocks.json` (see [`default_report_path`]).
    pub fn load(path: &Path) -> Result<Self, BlockDataError> {
        let text = std::fs::read_to_string(path).map_err(|e| BlockDataError::Io {
            path: path.display().to_string(),
            message: e.to_string(),
        })?;
        Self::from_report(&text)
    }

    /// Builds the table from the text of `blocks.json`, checking that every state id is where
    /// vanilla's state order puts it (ids contiguous from 0, blocks in registry order).
    pub fn from_report(json: &str) -> Result<Self, BlockDataError> {
        let report: BTreeMap<String, ReportBlock> =
            serde_json::from_str(json).map_err(|e| BlockDataError::Json(e.to_string()))?;
        let mut blocks: Vec<(&String, &ReportBlock)> = report.iter().collect();
        // The registry order is the order of the blocks' first state ids.
        let first = |b: &ReportBlock| b.states.iter().map(|s| s.id).min();
        if let Some((name, _)) = blocks.iter().find(|(_, b)| first(b).is_none()) {
            return Err(BlockDataError::NoDefault(name.to_string()));
        }
        blocks.sort_by_key(|(_, b)| first(b));

        let mut builder = BlockStatesBuilder::new();
        for (name, block) in blocks {
            let properties: Vec<(&str, Vec<&str>)> = block
                .properties
                .iter()
                .map(|(k, v)| (k.as_str(), v.iter().map(String::as_str).collect()))
                .collect();
            let borrowed: Vec<(&str, &[&str])> =
                properties.iter().map(|(k, v)| (*k, v.as_slice())).collect();
            let id = builder.add_block(name, &borrowed)?;
            let table = builder.table();
            let range = table.entry(id).first_state
                ..table.entry(id).first_state + table.entry(id).state_count;
            if block.states.len() != range.len() {
                return Err(BlockDataError::StateCount {
                    block: name.clone(),
                    count: block.states.len(),
                    expected: range.len() as u32,
                });
            }
            let mut default = None;
            for state in &block.states {
                let expected = (state.properties.len() == borrowed.len())
                    .then(|| {
                        state
                            .properties
                            .iter()
                            .try_fold(BlockState(range.start), |s, (k, v)| {
                                table.with_property(s, k, v)
                            })
                    })
                    .flatten();
                if expected != Some(BlockState(state.id)) {
                    return Err(BlockDataError::StateOrder {
                        block: name.clone(),
                        id: state.id,
                        expected: expected.map_or(u32::MAX, BlockState::id),
                    });
                }
                if state.default {
                    default = Some(BlockState(state.id));
                }
            }
            let default = default.ok_or_else(|| BlockDataError::NoDefault(name.clone()))?;
            builder.set_default(id, default)?;
        }
        Ok(builder.finish())
    }
}
