use axum::body::Body;
use axum::http::{Request, StatusCode};
use gitdiverge::daemon::{build_router, AppState, HealthResponse, WEBCLIENT_EMBEDDED};
use gitdiverge_lib::ProcessGitProvider;
use std::path::PathBuf;
use tower::ServiceExt;

fn app_state() -> AppState {
    let auth_config = gitdiverge::config::AuthConfig {
        enabled: false,
        mode: gitdiverge::config::AuthMode::Local {
            issuer: "test".to_string(),
            audience: "test".to_string(),
        },
    };
    AppState {
        clone_dir: PathBuf::from("."),
        git: ProcessGitProvider::new(),
        repo_locks: std::sync::Arc::new(gitdiverge::daemon::RepoLockManager::new()),
        auth: std::sync::Arc::new(gitdiverge::auth::AuthState::new(auth_config).unwrap()),
        webclient: gitdiverge::config::WebClientConfig::default(),
        divergence_cache: gitdiverge::divergence_cache::DivergenceCache::new(),
    }
}

#[tokio::test]
async fn health_endpoint_returns_ok() {
    let app = build_router(app_state());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
    let health: HealthResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(health.status, "ok");
}

#[tokio::test]
async fn openapi_json_endpoint_returns_ok() {
    let app = build_router(app_state());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api-docs/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json.get("paths").unwrap().get("/health").is_some());
    assert!(json
        .get("paths")
        .unwrap()
        .get("/api/v1/repos/{repo_guid}/branches")
        .is_some());
    assert!(json
        .get("paths")
        .unwrap()
        .get("/api/v1/repos/clone")
        .is_some());
}

#[tokio::test]
async fn swagger_ui_endpoint_returns_ok() {
    let app = build_router(app_state());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/swagger-ui/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn divergence_stream_returns_sse_for_missing_repo() {
    let app = build_router(app_state());
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/repos/nonexistent-guid/divergence?branches=main")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let content_type = response
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(content_type.contains("text/event-stream"));

    let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(text.contains("event:"));
    assert!(text.contains("data:"));
}

fn app_state_with_auth() -> AppState {
    let auth_config = gitdiverge::config::AuthConfig {
        enabled: true,
        mode: gitdiverge::config::AuthMode::Local {
            issuer: "test-issuer".to_string(),
            audience: "account".to_string(),
        },
    };
    AppState {
        clone_dir: PathBuf::from("."),
        git: ProcessGitProvider::new(),
        repo_locks: std::sync::Arc::new(gitdiverge::daemon::RepoLockManager::new()),
        auth: std::sync::Arc::new(gitdiverge::auth::AuthState::new(auth_config).unwrap()),
        webclient: gitdiverge::config::WebClientConfig::default(),
        divergence_cache: gitdiverge::divergence_cache::DivergenceCache::new(),
    }
}

#[tokio::test]
async fn test_token_endpoint_returns_bearer_token() {
    let app = build_router(app_state_with_auth());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/auth/test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["token_type"], "Bearer");
    assert!(!json["access_token"].as_str().unwrap().is_empty());
}

#[tokio::test]
async fn protected_endpoint_returns_401_without_token_when_auth_enabled() {
    let app = build_router(app_state_with_auth());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/repos")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn protected_endpoint_returns_200_with_valid_test_token() {
    let state = app_state_with_auth();
    let token = gitdiverge::auth::generate_test_token(&state.auth).unwrap();

    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/repos")
                .header("Authorization", format!("Bearer {}", token.access_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn protected_endpoint_returns_401_with_non_bearer_auth_header() {
    let app = build_router(app_state_with_auth());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/repos")
                .header("Authorization", "Basic dXNlcjpwYXNz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn protected_endpoint_returns_401_with_invalid_token() {
    let app = build_router(app_state_with_auth());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/repos")
                .header("Authorization", "Bearer invalid-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_token_endpoint_returns_403_in_jwks_mode() {
    let auth_config = gitdiverge::config::AuthConfig {
        enabled: true,
        mode: gitdiverge::config::AuthMode::Jwks {
            url: "https://example.com/jwks".to_string(),
            issuer: "test".to_string(),
            audience: "test".to_string(),
        },
    };
    let state = AppState {
        clone_dir: PathBuf::from("."),
        git: ProcessGitProvider::new(),
        repo_locks: std::sync::Arc::new(gitdiverge::daemon::RepoLockManager::new()),
        auth: std::sync::Arc::new(gitdiverge::auth::AuthState::new(auth_config).unwrap()),
        webclient: gitdiverge::config::WebClientConfig::default(),
        divergence_cache: gitdiverge::divergence_cache::DivergenceCache::new(),
    };
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/auth/test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn list_repos_returns_500_when_index_corrupted() {
    let tmp = tempfile::tempdir().unwrap();
    let clone_dir = tmp.path().join("repos");
    std::fs::create_dir(&clone_dir).unwrap();
    let gitdiverge_dir = clone_dir.join(".gitdiverge");
    std::fs::create_dir(&gitdiverge_dir).unwrap();
    std::fs::write(gitdiverge_dir.join("index.json"), "not json").unwrap();

    let mut state = app_state();
    state.clone_dir = clone_dir;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/repos")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn list_repos_returns_entries_when_index_has_repos() {
    let tmp = tempfile::tempdir().unwrap();
    let clone_dir = tmp.path().join("repos");
    std::fs::create_dir(&clone_dir).unwrap();
    let gitdiverge_dir = clone_dir.join(".gitdiverge");
    std::fs::create_dir(&gitdiverge_dir).unwrap();
    std::fs::write(
        gitdiverge_dir.join("index.json"),
        r#"{"https://example.com/repo.git":{"guid":"abc123","name":"repo","url":"https://example.com/repo.git","cloned_at":"2024-01-01T00:00:00Z"}}"#,
    )
    .unwrap();

    let mut state = app_state();
    state.clone_dir = clone_dir;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/repos")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let arr = json.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["guid"], "abc123");
}

#[tokio::test]
async fn list_branches_returns_400_when_repo_not_in_index() {
    let app = build_router(app_state());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/repos/nonexistent-guid/branches")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn list_branches_returns_400_when_repo_missing_on_disk() {
    let tmp = tempfile::tempdir().unwrap();
    let clone_dir = tmp.path().join("repos");
    std::fs::create_dir(&clone_dir).unwrap();
    let gitdiverge_dir = clone_dir.join(".gitdiverge");
    std::fs::create_dir(&gitdiverge_dir).unwrap();
    std::fs::write(
        gitdiverge_dir.join("index.json"),
        r#"{"https://example.com/repo.git":{"guid":"abc123","name":"repo","url":"https://example.com/repo.git","cloned_at":"2024-01-01T00:00:00Z"}}"#,
    )
    .unwrap();

    let mut state = app_state();
    state.clone_dir = clone_dir;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/repos/abc123/branches")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn list_branches_returns_branches_for_existing_repo() {
    let tmp = tempfile::tempdir().unwrap();
    let clone_dir = tmp.path().join("repos");
    std::fs::create_dir(&clone_dir).unwrap();
    let gitdiverge_dir = clone_dir.join(".gitdiverge");
    std::fs::create_dir(&gitdiverge_dir).unwrap();

    // Create a bare remote repo.
    let remote = clone_dir.join("remote.git");
    let status = std::process::Command::new("git")
        .args(["init", "--bare", remote.to_str().unwrap()])
        .status()
        .unwrap();
    assert!(status.success());

    // Create local repo, commit, and push two branches to the remote.
    let local = clone_dir.join("local");
    let status = std::process::Command::new("git")
        .args(["init", local.to_str().unwrap()])
        .status()
        .unwrap();
    assert!(status.success());

    std::fs::write(local.join("a.txt"), "a").unwrap();
    let status = std::process::Command::new("git")
        .current_dir(&local)
        .args(["add", "."])
        .status()
        .unwrap();
    assert!(status.success());
    let status = std::process::Command::new("git")
        .current_dir(&local)
        .args(["commit", "-m", "init"])
        .status()
        .unwrap();
    assert!(status.success());

    let status = std::process::Command::new("git")
        .current_dir(&local)
        .args(["checkout", "-b", "feature-x"])
        .status()
        .unwrap();
    assert!(status.success());

    let status = std::process::Command::new("git")
        .current_dir(&local)
        .args(["remote", "add", "origin", remote.to_str().unwrap()])
        .status()
        .unwrap();
    assert!(status.success());

    let status = std::process::Command::new("git")
        .current_dir(&local)
        .args(["push", "-u", "origin", "master", "feature-x"])
        .status()
        .unwrap();
    assert!(status.success());

    // Clone into the target path that the index will point to.
    let repo_path = clone_dir.join("abc123").join("repo");
    let status = std::process::Command::new("git")
        .args([
            "clone",
            remote.to_str().unwrap(),
            repo_path.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success());

    // Write the index so the daemon can resolve the GUID.
    std::fs::write(
        gitdiverge_dir.join("index.json"),
        format!(
            r#"{{"https://example.com/repo.git":{{"guid":"abc123","name":"repo","url":"https://example.com/repo.git","cloned_at":"2024-01-01T00:00:00Z"}}}}"#
        ),
    )
    .unwrap();

    let mut state = app_state();
    state.clone_dir = clone_dir;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/repos/abc123/branches")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let branches = json["branches"].as_array().unwrap();
    let names: Vec<String> = branches
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(names.contains(&"master".to_string()));
    assert!(names.contains(&"feature-x".to_string()));
}

#[tokio::test]
async fn clone_repo_returns_sse_already_exists() {
    let tmp = tempfile::tempdir().unwrap();
    let clone_dir = tmp.path().join("repos");
    std::fs::create_dir(&clone_dir).unwrap();
    let gitdiverge_dir = clone_dir.join(".gitdiverge");
    std::fs::create_dir(&gitdiverge_dir).unwrap();
    std::fs::write(
        gitdiverge_dir.join("index.json"),
        r#"{"https://example.com/repo.git":{"guid":"abc123","name":"repo","url":"https://example.com/repo.git","cloned_at":"2024-01-01T00:00:00Z"}}"#,
    )
    .unwrap();
    let repo_path = clone_dir.join("abc123").join("repo");
    std::fs::create_dir_all(&repo_path).unwrap();
    std::fs::create_dir(repo_path.join(".git")).unwrap();

    let mut state = app_state();
    state.clone_dir = clone_dir;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/repos/clone")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"url":"https://example.com/repo.git","force":false}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let content_type = response
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(content_type.contains("text/event-stream"));
}

#[tokio::test]
async fn clone_repo_failed_does_not_add_to_index() {
    let tmp = tempfile::tempdir().unwrap();
    let clone_dir = tmp.path().join("repos");
    std::fs::create_dir(&clone_dir).unwrap();
    let gitdiverge_dir = clone_dir.join(".gitdiverge");
    std::fs::create_dir(&gitdiverge_dir).unwrap();
    std::fs::write(gitdiverge_dir.join("index.json"), r#"{}"#).unwrap();

    let mut state = app_state();
    state.clone_dir = clone_dir.clone();
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/repos/clone")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"url":"not-a-valid-url:///foo","force":false}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let content_type = response
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(content_type.contains("text/event-stream"));

    let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(text.contains("event:error"));

    let index_content = std::fs::read_to_string(gitdiverge_dir.join("index.json")).unwrap();
    let index: serde_json::Value = serde_json::from_str(&index_content).unwrap();
    assert!(index.as_object().unwrap().is_empty());
}

#[tokio::test]
async fn protected_endpoint_returns_200_when_auth_disabled() {
    let app = build_router(app_state());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/repos")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn static_root_serves_index_html() {
    let app = build_router(app_state());
    let response = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();

    if !WEBCLIENT_EMBEDDED {
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        return;
    }

    assert_eq!(response.status(), StatusCode::OK);
    let content_type = response
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert_eq!(content_type, "text/html");
    let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(text.contains("GitDiverge"));
}

#[tokio::test]
async fn static_file_serves_with_correct_mime_type() {
    let app = build_router(app_state());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/favicon.svg")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    if !WEBCLIENT_EMBEDDED {
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        return;
    }

    assert_eq!(response.status(), StatusCode::OK);
    let content_type = response
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert_eq!(content_type, "image/svg+xml");
}

#[tokio::test]
async fn static_missing_path_falls_back_to_index_html() {
    let app = build_router(app_state());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/some/react/route")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    if !WEBCLIENT_EMBEDDED {
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        return;
    }

    assert_eq!(response.status(), StatusCode::OK);
    let content_type = response
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert_eq!(content_type, "text/html");
    let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(text.contains("GitDiverge"));
}

#[tokio::test]
async fn static_path_traversal_returns_404() {
    let app = build_router(app_state());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/../Cargo.toml")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn config_js_is_intercepted_and_returns_runtime_values() {
    let mut state = app_state_with_auth();
    state.webclient = gitdiverge::config::WebClientConfig {
        api_base: "http://test-api:9000".to_string(),
        oidc_authority: Some("https://auth.test/realms/app".to_string()),
        oidc_client_id: "test-client".to_string(),
        jira_server_addr: "https://jira.test".to_string(),
    };
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/config.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let content_type = response
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert_eq!(content_type, "application/javascript");
    let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(text.contains("http://test-api:9000"));
    assert!(text.contains("https://auth.test/realms/app"));
    assert!(text.contains("test-client"));
    assert!(text.contains("USE_AUTH: true"));
    assert!(text.contains("https://jira.test"));
}

#[tokio::test]
async fn config_js_reflects_auth_disabled() {
    let mut state = app_state();
    state.webclient = gitdiverge::config::WebClientConfig {
        api_base: "".to_string(),
        oidc_authority: None,
        oidc_client_id: "react-spa".to_string(),
        jira_server_addr: "".to_string(),
    };
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/config.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(text.contains("USE_AUTH: false"));
}

#[tokio::test]
async fn get_divergence_commits_returns_404_when_not_cached() {
    let app = build_router(app_state());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/repos/nonexistent-guid/divergence/commits?source_branch=main&target_branch=dev&branches=main,dev&page=0&page_size=2")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn get_divergence_commits_returns_paged_commits_from_cache() {
    let state = app_state();
    let analytics = gitdiverge_lib::BranchAnalytics {
        repo_path: "/tmp/repo".to_string(),
        branches: vec!["main".to_string(), "dev".to_string()],
        comparisons: vec![gitdiverge_lib::BranchComparison {
            source_branch: "dev".to_string(),
            target_branch: "main".to_string(),
            missing_commits: vec![
                gitdiverge_lib::Commit {
                    hash: "a1".to_string(),
                    author: gitdiverge_lib::Author {
                        name: "A".to_string(),
                        email: "a@example.com".to_string(),
                    },
                    timestamp: chrono::Utc::now(),
                    subject: "First".to_string(),
                    body: None,
                    parents: vec![],
                },
                gitdiverge_lib::Commit {
                    hash: "a2".to_string(),
                    author: gitdiverge_lib::Author {
                        name: "B".to_string(),
                        email: "b@example.com".to_string(),
                    },
                    timestamp: chrono::Utc::now(),
                    subject: "Second".to_string(),
                    body: None,
                    parents: vec![],
                },
                gitdiverge_lib::Commit {
                    hash: "a3".to_string(),
                    author: gitdiverge_lib::Author {
                        name: "C".to_string(),
                        email: "c@example.com".to_string(),
                    },
                    timestamp: chrono::Utc::now(),
                    subject: "Third".to_string(),
                    body: None,
                    parents: vec![],
                },
            ],
        }],
    };
    state.divergence_cache.insert_with_branches(
        "cached-guid".to_string(),
        &["main".to_string(), "dev".to_string()],
        analytics,
    );

    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/repos/cached-guid/divergence/commits?source_branch=dev&target_branch=main&branches=main,dev&page=0&page_size=2")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["total_commits"], 3);
    assert_eq!(json["commits"].as_array().unwrap().len(), 2);
    assert_eq!(json["commits"][0]["hash"], "a1");
    assert_eq!(json["commits"][1]["hash"], "a2");
}

#[tokio::test]
async fn get_divergence_commits_returns_empty_for_out_of_bounds_page() {
    let state = app_state();
    let analytics = gitdiverge_lib::BranchAnalytics {
        repo_path: "/tmp/repo".to_string(),
        branches: vec!["main".to_string()],
        comparisons: vec![gitdiverge_lib::BranchComparison {
            source_branch: "dev".to_string(),
            target_branch: "main".to_string(),
            missing_commits: vec![gitdiverge_lib::Commit {
                hash: "a1".to_string(),
                author: gitdiverge_lib::Author {
                    name: "A".to_string(),
                    email: "a@example.com".to_string(),
                },
                timestamp: chrono::Utc::now(),
                subject: "First".to_string(),
                body: None,
                parents: vec![],
            }],
        }],
    };
    state.divergence_cache.insert_with_branches(
        "cached-guid".to_string(),
        &["main".to_string()],
        analytics,
    );

    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/repos/cached-guid/divergence/commits?source_branch=dev&target_branch=main&branches=main&page=999&page_size=2")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["total_commits"], 1);
    assert!(json["commits"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn get_divergence_commits_returns_ok_when_query_contains_nonexistent_branch() {
    let state = app_state();
    let analytics = gitdiverge_lib::BranchAnalytics {
        repo_path: "/tmp/repo".to_string(),
        branches: vec!["main".to_string(), "dev".to_string()],
        comparisons: vec![gitdiverge_lib::BranchComparison {
            source_branch: "dev".to_string(),
            target_branch: "main".to_string(),
            missing_commits: vec![gitdiverge_lib::Commit {
                hash: "a1".to_string(),
                author: gitdiverge_lib::Author {
                    name: "A".to_string(),
                    email: "a@example.com".to_string(),
                },
                timestamp: chrono::Utc::now(),
                subject: "First".to_string(),
                body: None,
                parents: vec![],
            }],
        }],
    };
    // Simulate the behaviour of repo_divergence when some requested branches
    // do not exist: the analytics are stored under both the valid branch key
    // and the original requested branch key.
    state.divergence_cache.insert_with_branches(
        "cached-guid".to_string(),
        &["main".to_string(), "dev".to_string()],
        analytics.clone(),
    );
    state.divergence_cache.insert_with_branches(
        "cached-guid".to_string(),
        &["main".to_string(), "dev".to_string(), "fake".to_string()],
        analytics,
    );

    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/repos/cached-guid/divergence/commits?source_branch=dev&target_branch=main&branches=main,dev,fake&page=0&page_size=2")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["total_commits"], 1);
    assert_eq!(json["commits"].as_array().unwrap().len(), 1);
    assert_eq!(json["commits"][0]["hash"], "a1");
}

#[tokio::test]
async fn get_divergence_commits_returns_401_without_token_when_auth_enabled() {
    let app = build_router(app_state_with_auth());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/repos/some-guid/divergence/commits?source_branch=main&target_branch=dev&branches=main,dev&page=0&page_size=2")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
