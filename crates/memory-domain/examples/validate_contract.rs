//! Test bridge: exercise Rust deserialization against the same JSON as TypeScript.
use memory_domain::contracts::{Command, WorkBrief, WorkResult};
use memory_domain::workspace::{RenderManifest, WorkspaceState};
use std::io::{self, Read};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let value = match std::env::args().nth(1).as_deref() {
        Some("work-command") => {
            let command: Command<WorkBrief> = serde_json::from_str(&input)?;
            command.validate().map_err(|error| format!("{error:?}"))?;
            serde_json::to_value(command)?
        }
        Some("work-result") => {
            let result: WorkResult = serde_json::from_str(&input)?;
            result.validate().map_err(|error| format!("{error:?}"))?;
            serde_json::to_value(result)?
        }
        Some("workspace-state") => {
            serde_json::to_value(serde_json::from_str::<WorkspaceState>(&input)?)?
        }
        Some("render-manifest") => {
            serde_json::to_value(serde_json::from_str::<RenderManifest>(&input)?)?
        }
        _ => {
            return Err(
                "Expected work-command, work-result, workspace-state or render-manifest".into(),
            );
        }
    };
    println!("{}", serde_json::to_string(&value)?);
    Ok(())
}
