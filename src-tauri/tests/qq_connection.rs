use shixu_core::runtime::workers::WorkerPorts;
use shixu_core::{
    contracts::{AppResult, error::AppError},
    storage::{DataProtector, Database},
};
use shixu_desktop::{
    app_state::AppState,
    commands::{CallingContext, dispatch},
};
use std::{
    net::TcpListener,
    sync::Arc,
    time::{Duration, Instant},
};
struct Synthetic;
impl DataProtector for Synthetic {
    fn protect(&self, p: &[u8]) -> AppResult<Vec<u8>> {
        Ok(p.iter().map(|b| b ^ 0xa5).collect())
    }
    fn unprotect(&self, p: &[u8]) -> AppResult<Vec<u8>> {
        self.protect(p)
    }
}
fn state() -> AppState {
    AppState::from_database(Arc::new(
        Database::open(std::path::Path::new(":memory:"), Arc::new(Synthetic)).unwrap(),
    ))
}
fn main_context() -> CallingContext<'static> {
    CallingContext {
        label: "main",
        origin: "http://tauri.localhost",
    }
}
const ID: &str = "11111111-1111-4111-8111-111111111111";
fn source(state: &AppState) {
    dispatch(&main_context(),"save_source_config",serde_json::json!({"config":{"source_id":ID,"adapter_type":"onebot11-text","account_id":"42","allowed_group_ids":["7"],"timezone":"Asia/Shanghai","enabled":true,"capability_set":["live_messages"]}}),state).unwrap();
}
#[test]
fn protected_saved_connection_is_write_only_main_only_and_not_auto_connected() {
    let s = state();
    source(&s);
    let payload = serde_json::json!({"source_id":ID,"endpoint":"127.0.0.1:32123","token":b"synthetic-token".to_vec()});
    assert_eq!(
        dispatch(
            &CallingContext {
                label: "vault",
                origin: "http://tauri.localhost"
            },
            "qq_connection_save",
            payload.clone(),
            &s
        ),
        Err(AppError::AuthFailed)
    );
    assert_eq!(
        dispatch(
            &CallingContext {
                label: "main",
                origin: "https://remote.test"
            },
            "qq_connection_save",
            payload.clone(),
            &s
        ),
        Err(AppError::AuthFailed)
    );
    dispatch(&main_context(), "qq_connection_save", payload, &s).unwrap();
    let metadata = dispatch(
        &main_context(),
        "qq_connection_read",
        serde_json::json!({"source_id":ID}),
        &s,
    )
    .unwrap();
    assert_eq!(metadata["endpoint"], "127.0.0.1:32123");
    assert_eq!(metadata["has_credential"], true);
    assert_eq!(metadata["active"], false);
    assert!(metadata.get("token").is_none());
    for endpoint in [
        "localhost:1",
        "192.0.2.1:1",
        "http://127.0.0.1:1",
        "127.0.0.1:0",
    ] {
        assert_eq!(
            dispatch(
                &main_context(),
                "qq_connection_save",
                serde_json::json!({"source_id":ID,"endpoint":endpoint,"token":[65]}),
                &s
            ),
            Err(AppError::InvalidInput)
        );
    }
    for token in [
        vec![],
        vec![0],
        vec![13, 10],
        vec![128],
        vec![32],
        vec![65; 4097],
    ] {
        assert_eq!(
            dispatch(
                &main_context(),
                "qq_connection_save",
                serde_json::json!({"source_id":ID,"endpoint":"127.0.0.1:1","token":token}),
                &s
            ),
            Err(AppError::InvalidInput)
        );
    }
}
#[allow(clippy::result_large_err)]
#[test]
fn production_saved_socket_connect_persists_calendar_and_explicit_disconnect() {
    socket_calendar_for_groups(vec!["7"]);
}
#[test]
fn production_socket_accepts_descending_and_duplicate_saved_whitelists() {
    socket_calendar_for_groups(vec!["9", "7"]);
    socket_calendar_for_groups(vec!["7", "7"]);
}
#[allow(clippy::result_large_err)]
fn socket_calendar_for_groups(groups: Vec<&str>) {
    let s = state();
    source(&s);
    let mut config = s.settings().unwrap().sources().unwrap()[0].config.clone();
    config.allowed_group_ids = groups.iter().map(|g| (*g).to_owned()).collect();
    dispatch(
        &main_context(),
        "save_source_config",
        serde_json::json!({"config":config}),
        &s,
    )
    .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = listener.local_addr().unwrap().to_string();
    let (send_notice, notice) = std::sync::mpsc::channel();
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut ws = tungstenite::accept_hdr(
            stream,
            |r: &tungstenite::handshake::server::Request, response| {
                assert_eq!(r.headers()["authorization"], "Bearer synthetic-token");
                Ok(response)
            },
        )
        .unwrap();
        ws.send(tungstenite::Message::Text(serde_json::json!({"self_id":42,"post_type":"meta_event","meta_event_type":"lifecycle"}).to_string().into())).unwrap();
        notice.recv_timeout(Duration::from_secs(3)).unwrap();
        for group in [8, 9] {
            ws.send(tungstenite::Message::Text(serde_json::json!({"self_id":42,"post_type":"message","message_type":"group","group_id":group,"message_id":group,"user_id":88,"time":1791504000i64,"message":[{"type":"text","data":{"text":"普通说明"}}]}).to_string().into())).unwrap();
        }
        ws.send(tungstenite::Message::Text(serde_json::json!({"self_id":42,"post_type":"message","message_type":"group","group_id":7,"message_id":123,"user_id":88,"time":1791504000i64,"message":[{"type":"text","data":{"text":"2026年10月12日9:00高数考试"}}]}).to_string().into())).unwrap();
        while ws.read().is_ok() {}
    });
    let runtime = s.runtime().unwrap();
    runtime.resume().unwrap();
    let workers = runtime
        .spawn_workers(WorkerPorts {
            receiver: Some(s.take_qq_receiver().unwrap()),
            ..Default::default()
        })
        .unwrap();
    dispatch(
        &main_context(),
        "qq_connection_save",
        serde_json::json!({"source_id":ID,"endpoint":endpoint,"token":b"synthetic-token".to_vec()}),
        &s,
    )
    .unwrap();
    dispatch(
        &main_context(),
        "qq_connect",
        serde_json::json!({"source_id":ID}),
        &s,
    )
    .unwrap();
    struct Desktop;
    impl shixu_desktop::lifecycle::DesktopLifecycle for Desktop {
        fn hide_main(&self) -> AppResult<()> {
            Ok(())
        }
        fn focus_main(&self) -> AppResult<()> {
            Ok(())
        }
        fn exit(&self) -> AppResult<()> {
            Ok(())
        }
    }
    let lifecycle = shixu_desktop::lifecycle::Lifecycle {
        supervisor: runtime.clone(),
        vault: Arc::new(shixu_desktop::lifecycle::UnavailableVault),
        desktop: Arc::new(Desktop),
    };
    assert_eq!(
        lifecycle.handle_lifecycle(shixu_desktop::lifecycle::LifecycleEvent::WindowClose, 0),
        Err(AppError::Unsupported)
    );
    assert_eq!(
        lifecycle
            .vault
            .lock(shixu_core::contracts::vault::LockReason::Manual),
        Err(AppError::Unsupported)
    );
    assert!(runtime.status().running);
    send_notice.send(()).unwrap();
    let until = Instant::now() + Duration::from_secs(3);
    loop {
        let events=dispatch(&main_context(),"calendar_query",serde_json::json!({"query":{"from_date":null,"through_date":null,"statuses":["active"],"include_pending":true}}),&s).unwrap();
        if events.as_array().unwrap().len() == 1 {
            break;
        }
        assert!(
            Instant::now() < until,
            "saved production socket did not apply Calendar"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let messages = s.messages().unwrap().list(None, 100).unwrap();
    let mut received: Vec<_> = messages.iter().map(|m| m.group_id.as_str()).collect();
    received.sort();
    received.dedup();
    let mut expected = groups.clone();
    expected.sort();
    expected.dedup();
    assert_eq!(received, expected, "only exact authorized groups persist");
    assert_eq!(
        s.settings().unwrap().sources().unwrap()[0]
            .config
            .allowed_group_ids,
        expected
    );
    dispatch(
        &main_context(),
        "qq_disconnect",
        serde_json::json!({"source_id":ID}),
        &s,
    )
    .unwrap();
    assert_eq!(
        dispatch(
            &main_context(),
            "qq_connection_read",
            serde_json::json!({"source_id":ID}),
            &s
        )
        .unwrap()["active"],
        false
    );
    workers.stop().unwrap();
    server.join().unwrap();
    assert_eq!(
        dispatch(
            &main_context(),
            "qq_connect",
            serde_json::json!({"source_id":ID}),
            &s
        ),
        Err(AppError::Disconnected)
    );
}
#[test]
fn disabled_source_metadata_does_not_pretend_enabled_to_decrypt() {
    let s = state();
    source(&s);
    dispatch(
        &main_context(),
        "qq_connection_save",
        serde_json::json!({"source_id":ID,"endpoint":"127.0.0.1:32123","token":[65]}),
        &s,
    )
    .unwrap();
    let settings = dispatch(&main_context(), "settings_read", serde_json::json!({}), &s).unwrap();
    let mut c = settings["sources"][0]["config"].clone();
    c["enabled"] = false.into();
    dispatch(
        &main_context(),
        "save_source_config",
        serde_json::json!({"config":c}),
        &s,
    )
    .unwrap();
    let m = dispatch(
        &main_context(),
        "qq_connection_read",
        serde_json::json!({"source_id":ID}),
        &s,
    )
    .unwrap();
    assert_eq!(m["has_credential"], true);
    assert_eq!(m["endpoint"], serde_json::Value::Null);
    assert_eq!(m["active"], false);
}
