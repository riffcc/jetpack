// Jetpack
// Copyright (C) Riff Labs Limited <team@riff.cc>

//! HTTP-level tests for the native Gravity client. Uses wiremock to stand up a
//! fake Gravity API and assert the upsert/delete/idempotency/error behaviours
//! without touching a real server.

use jetpack::dns::gravity::GravityConfig;
use jetpack::dns::{DnsConfig, DnsSourceOfTruth};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn gravity_config(api_url: &str) -> DnsConfig {
    DnsConfig {
        path: ".".to_string(),
        zone: None,
        source_of_truth: DnsSourceOfTruth::Inventory,
        auto_sync: true,
        aliases: Default::default(),
        reverse_zone: None,
        gravity: Some(GravityConfig {
            api_url: api_url.to_string(),
            api_token: Some("test-token".to_string()),
            api_token_file: None,
            api_token_env: None,
            default_ttl: None,
        }),
    }
}

#[tokio::test]
async fn replace_records_is_a_noop_when_values_already_match() {
    let server = MockServer::start().await;
    let cfg = gravity_config(&server.uri());

    Mock::given(method("GET"))
        .and(path("/api/v1/dns/zones/records"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "records": [{ "hostname": "foo", "type": "A", "data": "10.0.0.1", "uid": "u1" }]
        })))
        .mount(&server)
        .await;

    let result = jetpack::dns::gravity::replace_records_async(
        &cfg, "example.com", "foo", "A", &["10.0.0.1"],
    )
    .await;
    assert!(result.is_ok(), "replace failed: {:?}", result.err());

    // Only the list (GET) should have fired — no DELETE, no POST.
    let received = server.received_requests().await.expect("request recording");
    assert_eq!(
        received.len(),
        1,
        "idempotent replace must not DELETE or POST"
    );
}

#[tokio::test]
async fn replace_records_deletes_old_and_posts_new_when_values_differ() {
    let server = MockServer::start().await;
    let cfg = gravity_config(&server.uri());

    Mock::given(method("GET"))
        .and(path("/api/v1/dns/zones/records"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "records": [{ "hostname": "foo", "type": "A", "data": "10.0.0.1", "uid": "u1" }]
        })))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/api/v1/dns/zones/records"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/dns/zones/records"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let result = jetpack::dns::gravity::replace_records_async(
        &cfg, "example.com", "foo", "A", &["10.0.0.2"],
    )
    .await;
    assert!(result.is_ok(), "replace failed: {:?}", result.err());

    // replace_records lists to check idempotency, then delete_record_async
    // lists again to find uids — so: GET + GET + DELETE (old) + POST (new) = 4.
    let received = server.received_requests().await.expect("request recording");
    assert_eq!(received.len(), 4);

    // The POST must target the FQDN zone (trailing dot) and carry a ttl in the
    // body — Gravity rejects a bare zone with NOT_FOUND and a ttl-less body
    // with INVALID_ARGUMENT.
    let post = received
        .iter()
        .find(|r| r.method == "POST")
        .expect("a POST should have been sent");
    assert!(
        post.url
            .query_pairs()
            .any(|(k, v)| k.as_ref() == "zone" && v.as_ref() == "example.com."),
        "POST zone query must be the FQDN with trailing dot: {}",
        post.url
    );
    let body: serde_json::Value = serde_json::from_slice(&post.body).expect("POST body is JSON");
    assert_eq!(body["type"], "A", "POST body type");
    assert_eq!(body["data"], "10.0.0.2", "POST body data");
    assert_eq!(body["ttl"], 3600, "POST body must include default ttl");
}

#[tokio::test]
async fn zone_query_is_fqdn_with_trailing_dot_even_for_list() {
    // Regression for the london DNS no-op: Gravity answers `zone=example.com`
    // with NOT_FOUND; the query must be `zone=example.com.`. Verified on the
    // list path (the first call any operation makes).
    let server = MockServer::start().await;
    let cfg = gravity_config(&server.uri());

    Mock::given(method("GET"))
        .and(path("/api/v1/dns/zones/records"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "records": [{ "hostname": "foo", "type": "A", "data": "10.0.0.1", "uid": "u1" }]
        })))
        .mount(&server)
        .await;

    // Desired == existing → no-op, so only the list (GET) fires.
    let result = jetpack::dns::gravity::replace_records_async(
        &cfg, "example.com", "foo", "A", &["10.0.0.1"],
    )
    .await;
    assert!(result.is_ok(), "replace failed: {:?}", result.err());

    let received = server.received_requests().await.expect("request recording");
    let get = received
        .iter()
        .find(|r| r.method == "GET")
        .expect("a GET should have been sent");
    assert!(
        get.url
            .query_pairs()
            .any(|(k, v)| k.as_ref() == "zone" && v.as_ref() == "example.com."),
        "list zone query must be the FQDN with trailing dot: {}",
        get.url
    );
}

#[tokio::test]
async fn default_ttl_is_overridable_from_config() {
    let server = MockServer::start().await;
    let mut cfg = gravity_config(&server.uri());
    cfg.gravity = cfg.gravity.map(|mut g| {
        g.default_ttl = Some(120);
        g
    });

    Mock::given(method("GET"))
        .and(path("/api/v1/dns/zones/records"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "records": null })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/dns/zones/records"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let result = jetpack::dns::gravity::replace_records_async(
        &cfg, "example.com", "foo", "A", &["10.0.0.5"],
    )
    .await;
    assert!(result.is_ok(), "replace failed: {:?}", result.err());

    let received = server.received_requests().await.expect("request recording");
    let post = received.iter().find(|r| r.method == "POST").unwrap();
    let body: serde_json::Value = serde_json::from_slice(&post.body).unwrap();
    assert_eq!(body["ttl"], 120, "configured default_ttl must be used");
}

#[tokio::test]
async fn delete_record_deletes_every_matching_uid() {
    let server = MockServer::start().await;
    let cfg = gravity_config(&server.uri());

    Mock::given(method("GET"))
        .and(path("/api/v1/dns/zones/records"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "records": [
                { "hostname": "foo", "type": "A", "data": "10.0.0.1", "uid": "u1" },
                { "hostname": "foo", "type": "A", "data": "10.0.0.2", "uid": "u2" }
            ]
        })))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/api/v1/dns/zones/records"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let result =
        jetpack::dns::gravity::delete_record_async(&cfg, "example.com", "foo", "A").await;
    assert!(result.is_ok(), "delete failed: {:?}", result.err());

    // GET (list) + 2 DELETE (one per uid) = 3 calls.
    let received = server.received_requests().await.expect("request recording");
    assert_eq!(received.len(), 3);
}

#[tokio::test]
async fn non_success_response_is_propagated_as_an_error() {
    let server = MockServer::start().await;
    let cfg = gravity_config(&server.uri());

    Mock::given(method("GET"))
        .and(path("/api/v1/dns/zones/records"))
        .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
        .mount(&server)
        .await;

    let result = jetpack::dns::gravity::replace_records_async(
        &cfg, "example.com", "foo", "A", &["10.0.0.1"],
    )
    .await;
    let err = result.expect_err("expected an error");
    assert!(
        err.contains("list-records"),
        "error should mention list-records: {}",
        err
    );
}
