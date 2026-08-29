//! `gdmerge diff`

use std::path::Path;

use anyhow::{Context, Result};
use tscn::{Change, Document};

use crate::io;
use crate::show::shown;

pub fn run(before: &Path, after: &Path, json: bool) -> Result<i32> {
    let a_src = io::read(before)?;
    let b_src = io::read(after)?;
    let a = Document::parse(&a_src).with_context(|| format!("parsing {}", before.display()))?;
    let b = Document::parse(&b_src).with_context(|| format!("parsing {}", after.display()))?;
    let d = tscn::diff(&a, &b);

    if json {
        println!("{}", serde_json::to_string_pretty(&d)?);
        return Ok(0);
    }

    if d.is_empty() {
        println!("no semantic changes");
        return Ok(0);
    }

    let n = d.changes.len();
    println!("{n} semantic change{}", if n == 1 { "" } else { "s" });
    for change in &d.changes {
        println!();
        match change {
            Change::Added { entity, .. } => println!("  + {entity}"),
            Change::Removed { entity, .. } => println!("  - {entity}"),
            Change::Moved { from, to, .. } => println!("  > {from} -> {to}"),
            Change::Reordered { entity, from, to, .. } => {
                println!("  ~ {entity} moved from position {from} to {to}");
            }
            Change::Modified { entity, fields, properties, .. } => {
                println!("  ~ {entity}");
                for c in fields.iter().chain(properties) {
                    match (&c.before, &c.after) {
                        (Some(b), Some(a)) => {
                            println!("      {}: {} -> {}", c.key, shown(b), shown(a))
                        }
                        (None, Some(a)) => println!("      + {}: {}", c.key, shown(a)),
                        (Some(b), None) => println!("      - {}: {}", c.key, shown(b)),
                        (None, None) => {}
                    }
                }
            }
        }
    }
    Ok(0)
}
