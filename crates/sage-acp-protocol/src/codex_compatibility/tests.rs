use serde_json::{Value, json};

use super::schema::{Evidence, SchemaBuild};
use super::*;

fn replace_definition(evidence: &mut Evidence, build: &mut SchemaBuild, name: &str, shape: Value) {
    build
        .definitions
        .insert(name.to_owned(), evidence.schemas.len());
    evidence.schemas.push(shape);
}

#[test]
fn newly_constrained_request_config_members_degrade_the_build() {
    let mut evidence = Evidence::recorded();
    let mut build = evidence.builds[2].clone();
    let mut shape = evidence.schemas[build.definitions["ThreadStartParams"]].clone();
    // Sage sends an object here when starting a browser-enabled session.
    shape["properties"]["config"]["properties"] = json!({"mcp_servers":{"type":"integer"}});
    replace_definition(&mut evidence, &mut build, "ThreadStartParams", shape);
    assert!(
        evidence
            .failures(&build)
            .contains(&"client:thread/start".to_owned())
    );
}

#[test]
fn added_request_properties_must_accept_the_baseline_catch_all_values() {
    for (previous, added, compatible) in [
        (json!({}), json!({"type":"integer"}), false),
        (json!({"properties":{}}), json!({"type":"integer"}), false),
        (json!({"additionalProperties":true}), json!({}), true),
        (
            json!({"additionalProperties":false}),
            json!({"type":"integer"}),
            true,
        ),
        (
            json!({"patternProperties":{"^mcp_":{"type":"object"}},"additionalProperties":false}),
            json!({"type":"integer"}),
            false,
        ),
        (
            json!({"patternProperties":{"^mcp_":{"type":"object"}},"additionalProperties":{"type":"integer"}}),
            json!({"type":"integer"}),
            false,
        ),
        (
            json!({"patternProperties":{"^mcp_":{"type":"object"}},"additionalProperties":false}),
            json!({}),
            true,
        ),
        (
            json!({"additionalProperties":{"type":"object"}}),
            json!({"type":"integer"}),
            false,
        ),
        (
            json!({"additionalProperties":{"type":"object"}}),
            json!({"type":"object"}),
            true,
        ),
    ] {
        let mut evidence = Evidence::recorded();
        let mut baseline = evidence.builds[1].clone();
        let mut shape = evidence.schemas[baseline.definitions["ThreadStartParams"]].clone();
        shape["properties"]["config"] = previous;
        replace_definition(
            &mut evidence,
            &mut baseline,
            "ThreadStartParams",
            shape.clone(),
        );
        evidence.builds[1] = baseline.clone();
        let mut candidate = baseline;
        shape["properties"]["config"]["properties"] = json!({"mcp_servers":added});
        replace_definition(&mut evidence, &mut candidate, "ThreadStartParams", shape);
        assert_eq!(
            !evidence
                .failures(&candidate)
                .contains(&"client:thread/start".to_owned()),
            compatible,
        );
    }
}

#[test]
fn optional_incoming_properties_remain_compatible() {
    for (name, method) in [
        ("Model", "response:model/list"),
        ("ThreadStartedNotification", "notification:thread/started"),
    ] {
        let mut evidence = Evidence::recorded();
        let mut build = evidence.builds[2].clone();
        let mut shape = evidence.schemas[build.definitions[name]].clone();
        shape["properties"]["futureMetadata"] = json!({"type":"integer"});
        replace_definition(&mut evidence, &mut build, name, shape);
        assert!(!evidence.failures(&build).contains(&method.to_owned()));
    }
}

#[test]
fn changed_function_call_output_shapes_degrade_the_build() {
    for (name, field, replacement) in [
        ("ThreadItem", "name", json!({"type":"integer"})),
        ("ThreadItem", "output", json!({"type":"integer"})),
        (
            "FunctionCallOutputContentItem",
            "text",
            json!({"type":"integer"}),
        ),
    ] {
        let mut evidence = Evidence::recorded();
        let mut build = evidence.builds[2].clone();
        let mut shape = evidence.schemas[build.definitions[name]].clone();
        let variant = shape["oneOf"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|variant| {
                variant["properties"]["type"]["enum"][0]
                    == if name == "ThreadItem" {
                        "functionCallOutput"
                    } else {
                        "input_text"
                    }
            })
            .expect("the evidence must retain consumed function-call-output fields");
        variant["properties"][field] = replacement;
        replace_definition(&mut evidence, &mut build, name, shape);
        assert!(
            evidence
                .failures(&build)
                .contains(&"notification:item/completed".to_owned()),
            "{name}.{field}"
        );
    }
}

#[test]
fn recorded_real_schemas_prove_the_advertised_capabilities() {
    let evidence = Evidence::recorded();
    for build in &evidence.builds {
        assert_eq!(build.schema_sha256.len(), 64);
        let failures = evidence.failures(build);
        let proven = failures.is_empty();
        assert_eq!(
            CodexCapabilities {
                session_extensions: proven,
                access_policy: proven,
                goals: proven,
                browser: proven
            },
            observed_capabilities(&build.version),
            "{}: {failures:?}",
            build.version
        );
    }
    assert_eq!(
        builds().len(),
        evidence.builds.len(),
        "every registry row needs real schema evidence"
    );
}

#[test]
fn incompatible_params_policy_or_transitive_all_of_shapes_cannot_prove_a_build() {
    for (name, shape, failed_method) in [
        (
            "TurnStartParams",
            json!({"properties":{"threadId":{"type":"integer"}},"required":["threadId"],"type":"object"}),
            "client:turn/start",
        ),
        (
            "SandboxPolicy",
            json!({"type":"string"}),
            "policy:SandboxPolicy",
        ),
        // InitializeResponse reaches this scalar through allOf.
        (
            "AbsolutePathBuf",
            json!({"type":"integer"}),
            "response:initialize",
        ),
    ] {
        let mut evidence = Evidence::recorded();
        let mut build = evidence.builds[2].clone();
        replace_definition(&mut evidence, &mut build, name, shape);
        assert!(
            evidence
                .failures(&build)
                .contains(&failed_method.to_owned()),
            "{name}"
        );
    }
}

#[test]
fn missing_turn_notifications_and_browser_calls_degrade_the_wire() {
    for method in [
        "notification:turn/started",
        "server:item/tool/call",
        "reply:item/tool/call",
    ] {
        let evidence = Evidence::recorded();
        let mut build = evidence.builds[2].clone();
        build.wire.remove(method);
        assert!(evidence.failures(&build).contains(&method.to_owned()));
    }
}

#[test]
fn new_incoming_variants_and_closed_object_fields_need_new_binding_support() {
    for (name, field, value, failed_method) in [
        (
            "ThreadNameUpdatedNotification",
            "properties",
            json!({"threadId":{"type":"string"},"threadName":{"type":"string"},"timestamp":{"type":"integer"}}),
            "notification:thread/name/updated",
        ),
        (
            "ThreadGoalClearedNotification",
            "properties",
            json!({"threadId":{"type":"string"},"reason":{"type":"string"}}),
            "notification:thread/goal/cleared",
        ),
        (
            "AccountLoginCompletedNotification",
            "properties",
            json!({"newField":{"type":"string"}}),
            "notification:account/login/completed",
        ),
    ] {
        let mut evidence = Evidence::recorded();
        let mut build = evidence.builds[2].clone();
        let mut shape = evidence.schemas[build.definitions[name]].clone();
        shape[field] = value;
        replace_definition(&mut evidence, &mut build, name, shape);
        assert!(
            evidence
                .failures(&build)
                .contains(&failed_method.to_owned())
        );
    }
    let mut evidence = Evidence::recorded();
    let mut build = evidence.builds[2].clone();
    let mut shape = evidence.schemas[build.definitions["ThreadStatus"]].clone();
    shape["oneOf"]
        .as_array_mut()
        .unwrap()
        .push(json!({"type":"integer"}));
    replace_definition(&mut evidence, &mut build, "ThreadStatus", shape);
    assert!(
        evidence
            .failures(&build)
            .contains(&"notification:thread/status/changed".to_owned())
    );
}

#[test]
fn added_notification_envelope_metadata_requires_binding_support() {
    let mut evidence = Evidence::recorded();
    let mut build = evidence.builds[2].clone();
    let mut envelope = evidence.schemas[build.wire["envelope:ServerNotification"]].clone();
    envelope["properties"]["extraTimestamp"] = json!({"type":"string"});
    build.wire.insert(
        "envelope:ServerNotification".to_owned(),
        evidence.schemas.len(),
    );
    evidence.schemas.push(envelope);
    assert!(
        evidence
            .failures(&build)
            .contains(&"envelope:ServerNotification".to_owned())
    );
}

#[test]
fn description_members_are_wire_fields_rather_than_schema_annotations() {
    let mut evidence = Evidence::recorded();
    let mut build = evidence.builds[2].clone();
    let mut model = evidence.schemas[build.definitions["Model"]].clone();
    model["properties"]["description"] = json!({"type":"integer"});
    replace_definition(&mut evidence, &mut build, "Model", model);
    assert!(
        evidence
            .failures(&build)
            .contains(&"response:model/list".to_owned())
    );
}

#[test]
fn recorded_observed_releases_have_full_capabilities_and_unproven_versions_do_not() {
    for version in ["0.167.0", "codex-cli 0.167.1"] {
        assert!(observed_capabilities(version).characterised(), "{version}");
        assert!(
            protocol_capabilities(&format!(
                "codex-app-server-observed-{}",
                version.split_ascii_whitespace().next_back().unwrap()
            ))
            .browser
        );
    }
    for version in [
        "",
        "codex-cli",
        "latest",
        "0.170.0",
        "codex-cli 0.170.0",
        "0.171.0",
        "99.0.0",
        "0.170.0-rc.1",
        "0.170.0+other",
        "0.170.0\u{7}",
    ] {
        assert_eq!(
            observed_capabilities(version),
            CodexCapabilities::default(),
            "{version}"
        );
    }
}

#[test]
fn release_0170_stays_degraded_for_new_constraints_on_open_requests() {
    let evidence = Evidence::recorded();
    let build = evidence
        .builds
        .iter()
        .find(|build| build.version == "0.170.0")
        .unwrap();
    assert_eq!(
        evidence.failures(build),
        ["client:model/list", "client:thread/list"]
    );
    assert_eq!(
        observed_capabilities(&build.version),
        CodexCapabilities::default()
    );
}
