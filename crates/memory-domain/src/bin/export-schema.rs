use memory_domain::contracts::{Command, CommandResponse, WorkBrief, WorkResult};
use memory_domain::coordination::{Assignment, HostRequest, HostResponse};
use memory_domain::workspace::{RenderManifest, WorkspaceState};
use schemars::{JsonSchema, generate::SchemaSettings, transform::RestrictFormats};
use std::{fs, path::Path};

fn write<T: JsonSchema>(directory: &Path, name: &str) -> Result<(), Box<dyn std::error::Error>> {
    fs::write(
        directory.join(name),
        serde_json::to_string_pretty(
            &SchemaSettings::draft2020_12()
                .with_transform(RestrictFormats::default())
                .into_generator()
                .into_root_schema_for::<T>(),
        )? + "\n",
    )?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contracts/generated");
    fs::create_dir_all(&directory)?;
    write::<Command<WorkBrief>>(&directory, "work-command.schema.json")?;
    write::<WorkResult>(&directory, "work-result.schema.json")?;
    write::<CommandResponse>(&directory, "command-response.schema.json")?;
    write::<memory_domain::consolidation::ConsolidationReview>(
        &directory,
        "consolidation-review.schema.json",
    )?;
    write::<memory_domain::consolidation::ConsolidationResult>(
        &directory,
        "consolidation-result.schema.json",
    )?;
    write::<memory_domain::consolidation::QualificationInput>(
        &directory,
        "qualification-input.schema.json",
    )?;
    write::<memory_domain::consolidation::QualificationReport>(
        &directory,
        "qualification-report.schema.json",
    )?;
    write::<memory_domain::consolidation::QualificationSuite>(
        &directory,
        "qualification-suite.schema.json",
    )?;
    write::<memory_domain::consolidation::AdoptionResult>(
        &directory,
        "adoption-result.schema.json",
    )?;
    write::<memory_domain::maintenance::MaintenanceReview>(
        &directory,
        "maintenance-review.schema.json",
    )?;
    write::<memory_domain::maintenance::MaintenanceResult>(
        &directory,
        "maintenance-result.schema.json",
    )?;
    write::<memory_domain::intentions::IntentionCheck>(&directory, "intention-check.schema.json")?;
    write::<memory_domain::intentions::IntentionOccurrence>(
        &directory,
        "intention-occurrence.schema.json",
    )?;
    write::<HostRequest>(&directory, "host-request.schema.json")?;
    write::<HostResponse>(&directory, "host-response.schema.json")?;
    write::<memory_domain::activation::ActivationWindow>(
        &directory,
        "activation-window.schema.json",
    )?;
    write::<memory_domain::activation::ContextPackage>(&directory, "context-package.schema.json")?;
    write::<Assignment>(&directory, "assignment.schema.json")?;
    write::<memory_domain::scoped::InvestigationPlan>(
        &directory,
        "investigation-plan.schema.json",
    )?;
    write::<memory_domain::formation::FormationWindow>(&directory, "formation-window.schema.json")?;
    write::<memory_domain::formation::FormationResult>(&directory, "formation-result.schema.json")?;
    write::<WorkspaceState>(&directory, "workspace-state.schema.json")?;
    write::<RenderManifest>(&directory, "render-manifest.schema.json")?;
    write::<memory_domain::judgement::JudgementPacket>(&directory, "judgement-packet.schema.json")?;
    write::<memory_domain::judgement::SemanticAssessment>(
        &directory,
        "semantic-assessment.schema.json",
    )?;
    write::<memory_domain::judgement::JudgementDecision>(
        &directory,
        "judgement-decision.schema.json",
    )?;
    write::<memory_domain::judgement::TaskLocalCheck>(&directory, "task-local-check.schema.json")?;
    Ok(())
}
