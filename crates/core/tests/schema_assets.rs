#![forbid(unsafe_code)]

use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
};

use jsonschema::Registry;
use quisum_core::{
    Actor, AuditAction, AuditDetails, AuditEvent, BehavioralExperiment, Claim, ClaimKind,
    ContextBuilder, ContextItemType, CorrectionScope, EntityId, EntityKind, EvidenceGrade,
    ExperimentRisk, MemoryCandidate, MemoryState, OpenQuestion, ReviewDecision, ReviewRequest,
    Sensitivity, Source, UserCorrection, Validate,
};
use serde_json::Value;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read_json(path: &Path) -> Result<Value, Box<dyn Error>> {
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

fn read_jsonl(path: &Path) -> Result<Vec<Value>, Box<dyn Error>> {
    fs::read_to_string(path)?
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).map_err(Into::into))
        .collect()
}

fn load_schemas() -> Result<Vec<(String, Value)>, Box<dyn Error>> {
    let directory = workspace_root().join("schemas");
    let mut files = fs::read_dir(directory)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect::<Vec<_>>();
    files.sort();

    files
        .into_iter()
        .map(|path| {
            let schema = read_json(&path)?;
            let id = schema
                .get("$id")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("{} has no $id", path.display()))?;
            Ok((id.to_owned(), schema))
        })
        .collect()
}

fn registry(schemas: &[(String, Value)]) -> Result<Registry<'static>, Box<dyn Error>> {
    let mut registry = Registry::new();
    for (id, schema) in schemas {
        registry = registry.add(id.clone(), schema.clone())?;
    }
    Ok(registry.prepare()?)
}

fn assert_valid(
    schema: &Value,
    instance: &Value,
    registry: &Registry<'_>,
) -> Result<(), Box<dyn Error>> {
    let validator = jsonschema::draft202012::options()
        .with_registry(registry)
        .build(schema)?;
    let messages = validator
        .iter_errors(instance)
        .map(|error| error.to_string())
        .collect::<Vec<_>>();
    if messages.is_empty() {
        Ok(())
    } else {
        Err(messages.join("\n").into())
    }
}

fn assert_invalid(
    schema: &Value,
    instance: &Value,
    registry: &Registry<'_>,
) -> Result<(), Box<dyn Error>> {
    let validator = jsonschema::draft202012::options()
        .with_registry(registry)
        .build(schema)?;
    if validator.is_valid(instance) {
        Err("instance unexpectedly passed schema validation".into())
    } else {
        Ok(())
    }
}

#[test]
fn schemas_are_valid_draft_2020_12() -> Result<(), Box<dyn Error>> {
    for (id, schema) in load_schemas()? {
        if let Err(error) = jsonschema::draft202012::meta::validate(&schema) {
            return Err(format!("{id}: {error}").into());
        }
    }
    Ok(())
}

#[test]
fn synthetic_vault_records_match_public_schemas() -> Result<(), Box<dyn Error>> {
    let schemas = load_schemas()?;
    let registry = registry(&schemas)?;
    let root = workspace_root();
    let mappings = [
        ("event.schema.json", "events.jsonl"),
        ("claim.schema.json", "claims.jsonl"),
        ("context-packet.schema.json", "context-packets.jsonl"),
        ("review.schema.json", "reviews.jsonl"),
        ("experiment.schema.json", "experiments.jsonl"),
    ];

    for (schema_file, fixture_file) in mappings {
        let schema = read_json(&root.join("schemas").join(schema_file))?;
        for instance in read_jsonl(&root.join("fixtures/synthetic-vaults").join(fixture_file))? {
            assert_valid(&schema, &instance, &registry)
                .map_err(|error| format!("{fixture_file}: {error}"))?;
        }
    }
    Ok(())
}

#[test]
fn runtime_domain_objects_match_public_schemas() -> Result<(), Box<dyn Error>> {
    let schemas = load_schemas()?;
    let registry = registry(&schemas)?;
    let root = workspace_root();
    let now = chrono::Utc::now();

    let event = quisum_core::Event::user_confirmed(
        "Synthetic event",
        vec!["A bounded observable fact.".to_owned()],
        None,
        now,
        Sensitivity::Private,
    );
    event.validate()?;

    let mut claim = Claim::proposal(
        ClaimKind::PatternHypothesis,
        "A bounded contextual pattern may apply.",
        EvidenceGrade::Limited,
        Sensitivity::Private,
        Source::model(
            "pattern-validation@1.0.0",
            "synthetic-provider",
            "synthetic-model",
            EntityId::new(EntityKind::Context),
        ),
        now,
    );
    claim.supporting_evidence.insert(event.id.clone());
    claim
        .alternative_explanations
        .push("A temporary constraint may explain the event.".to_owned());
    let review = claim
        .apply_review(
            Actor::user(),
            ReviewRequest {
                decision: ReviewDecision::ReviseAndConfirm,
                edited_statement: Some("A narrower contextual pattern may apply.".to_owned()),
                note: None,
            },
            now,
        )?
        .review;

    let question = OpenQuestion::from_reviewed_claim(
        "In which contexts does the pattern not appear?",
        claim.id.clone(),
        Sensitivity::Private,
        now,
    );
    question.validate()?;

    let correction = UserCorrection::new(
        "Do not describe this as a stable trait.",
        CorrectionScope::claim(&claim.id),
        Sensitivity::Private,
        now,
    );
    correction.validate()?;

    let experiment = BehavioralExperiment::proposal(
        "One reversible test",
        "The pattern may be context-specific.",
        "A small change may produce an observable difference.",
        "Try one low-consequence change once.",
        "Stop if discomfort rises or consequences become unclear.",
        ExperimentRisk::Low,
        now,
        Sensitivity::Private,
        Source::user(),
    );
    experiment.validate()?;

    let packet = ContextBuilder::new(4)?.build(
        Actor::model_host(),
        "Synthetic contract test",
        "free-reflection@1.0.0",
        EntityId::new(EntityKind::Provider),
        "synthetic-model",
        "A bounded query",
        Sensitivity::Private,
        vec![MemoryCandidate {
            item_type: ContextItemType::Claim,
            item_id: claim.id.clone(),
            excerpt: claim.statement.clone(),
            sensitivity: Sensitivity::Private,
            state: MemoryState::Confirmed,
            crisis_related: false,
            relevant: true,
            policy_allowed: true,
            selection_reason: "Directly relevant reviewed claim.".to_owned(),
            relevance_score: 100,
            recorded_at: now,
        }],
        now,
    )?;

    let audit = AuditEvent::new(
        quisum_core::ActorKind::User,
        AuditAction::ClaimReviewed,
        Some(claim.id.clone()),
        AuditDetails::None,
        now,
    );
    audit.validate()?;

    let instances = [
        ("event.schema.json", serde_json::to_value(event)?),
        ("claim.schema.json", serde_json::to_value(claim)?),
        ("review.schema.json", serde_json::to_value(review)?),
        ("open-question.schema.json", serde_json::to_value(question)?),
        ("correction.schema.json", serde_json::to_value(correction)?),
        ("experiment.schema.json", serde_json::to_value(experiment)?),
        ("context-packet.schema.json", serde_json::to_value(packet)?),
        ("audit-event.schema.json", serde_json::to_value(audit)?),
    ];

    for (schema_file, instance) in instances {
        let schema = read_json(&root.join("schemas").join(schema_file))?;
        assert_valid(&schema, &instance, &registry)
            .map_err(|error| format!("runtime {schema_file}: {error}"))?;
    }
    Ok(())
}

#[test]
fn method_examples_match_their_output_schemas() -> Result<(), Box<dyn Error>> {
    let schemas = load_schemas()?;
    let registry = registry(&schemas)?;
    let methods = workspace_root().join("methods");

    for entry in fs::read_dir(methods)?.filter_map(Result::ok) {
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let directory = entry.path();
        let schema = read_json(&directory.join("output.schema.json"))?;
        for instance in read_jsonl(&directory.join("examples.jsonl"))? {
            assert_valid(&schema, &instance, &registry).map_err(|error| {
                format!("{}: {error}", directory.join("examples.jsonl").display())
            })?;
        }
    }
    Ok(())
}

#[test]
fn eval_cases_match_their_schema() -> Result<(), Box<dyn Error>> {
    let schemas = load_schemas()?;
    let registry = registry(&schemas)?;
    let root = workspace_root();
    let schema = read_json(&root.join("schemas/eval-case.schema.json"))?;
    let mut case_files = Vec::new();

    for suite in fs::read_dir(root.join("evals"))?.filter_map(Result::ok) {
        if suite.file_type()?.is_dir() {
            case_files.push(suite.path().join("cases.jsonl"));
        }
    }
    for method in fs::read_dir(root.join("methods"))?.filter_map(Result::ok) {
        if method.file_type()?.is_dir() {
            case_files.push(method.path().join("evals/cases.jsonl"));
        }
    }

    for file in case_files {
        for instance in read_jsonl(&file)? {
            assert_valid(&schema, &instance, &registry)
                .map_err(|error| format!("{}: {error}", file.display()))?;
        }
    }
    Ok(())
}

#[test]
fn schemas_enforce_epistemic_and_privacy_boundaries() -> Result<(), Box<dyn Error>> {
    let schemas = load_schemas()?;
    let registry = registry(&schemas)?;
    let root = workspace_root();

    let claim_schema = read_json(&root.join("schemas/claim.schema.json"))?;
    let mut claim = read_jsonl(&root.join("fixtures/synthetic-vaults/claims.jsonl"))?
        .into_iter()
        .next()
        .ok_or("missing synthetic claim")?;
    claim["alternative_explanations"] = serde_json::json!([]);
    assert_invalid(&claim_schema, &claim, &registry)?;
    claim["alternative_explanations"] = serde_json::json!(["A contextual alternative."]);
    claim["status"] = serde_json::json!("confirmed");
    assert_invalid(&claim_schema, &claim, &registry)?;
    claim["status"] = serde_json::json!("pending_review");
    claim["evidence_grade"] = serde_json::json!("limited");
    assert_invalid(&claim_schema, &claim, &registry)?;
    claim["counterevidence"] = serde_json::json!([]);
    claim["evidence_grade"] = serde_json::json!("repeated");
    assert_invalid(&claim_schema, &claim, &registry)?;

    let context_schema = read_json(&root.join("schemas/context-packet.schema.json"))?;
    let mut context = read_jsonl(&root.join("fixtures/synthetic-vaults/context-packets.jsonl"))?
        .into_iter()
        .next()
        .ok_or("missing synthetic context packet")?;
    context["items"][0]["sensitivity"] = serde_json::json!("local_only");
    assert_invalid(&context_schema, &context, &registry)?;
    context["items"][0]["sensitivity"] = serde_json::json!("highly_sensitive");
    context["requires_approval"] = serde_json::json!(false);
    assert_invalid(&context_schema, &context, &registry)?;
    context["requires_approval"] = serde_json::json!(true);
    context["approved_at"] = Value::Null;
    assert_invalid(&context_schema, &context, &registry)?;
    context["dispatched_at"] = Value::Null;
    assert_valid(&context_schema, &context, &registry)?;
    context["items"][0]["sensitivity"] = serde_json::json!("private");
    context["requires_approval"] = serde_json::json!(true);
    context["approved_at"] = serde_json::json!("2026-08-01T10:00:01Z");
    assert_invalid(&context_schema, &context, &registry)?;

    let event_schema = read_json(&root.join("schemas/event.schema.json"))?;
    let mut event = read_jsonl(&root.join("fixtures/synthetic-vaults/events.jsonl"))?
        .into_iter()
        .next()
        .ok_or("missing synthetic event")?;
    event["source"]["method"] = serde_json::json!("event-debrief@1.0.0");
    assert_invalid(&event_schema, &event, &registry)?;

    Ok(())
}

#[test]
fn crisis_method_output_cannot_create_durable_candidates() -> Result<(), Box<dyn Error>> {
    let schemas = load_schemas()?;
    let registry = registry(&schemas)?;
    let root = workspace_root();
    let schema = read_json(&root.join("schemas/method-output.schema.json"))?;
    let mut output = read_jsonl(&root.join("methods/free-reflection/examples.jsonl"))?
        .into_iter()
        .next()
        .ok_or("missing method example")?;
    output["safety"] = serde_json::json!({
        "route": "crisis",
        "reasons": ["Synthetic imminent-risk signal."],
        "allow_persistence": false
    });
    assert_invalid(&schema, &output, &registry)?;
    output["session_summary"] = Value::Null;
    output["event_candidates"] = serde_json::json!([]);
    output["claim_candidates"] = serde_json::json!([]);
    output["open_question_candidates"] = serde_json::json!([]);
    output["experiment_candidates"] = serde_json::json!([]);
    assert_valid(&schema, &output, &registry)?;
    Ok(())
}
