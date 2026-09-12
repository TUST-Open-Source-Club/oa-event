//! event 集成测试：活动/表单/公开报名（验证码）/审核候补/签到/统计/导出。

mod common;

use axum::http::StatusCode;
use common::*;
use serde_json::json;
use uuid::Uuid;

/// 创建并开放活动。
async fn open_event(
    app: &TestApp,
    token: &str,
    slug: &str,
    capacity: i64,
    need_review: bool,
) -> String {
    let created = request(
        &app.app,
        "POST",
        "/api/v1/event/events",
        Some(token),
        Some(&json!({
            "slug": slug,
            "title": "迎新活动",
            "capacity": capacity,
            "needReview": need_review,
            "waitlistEnabled": true,
            "emailVerify": true
        })),
    )
    .await;
    let created = created.expect(StatusCode::CREATED);
    let event_id = created["id"].as_str().unwrap().to_string();
    request(
        &app.app,
        "PATCH",
        &format!("/api/v1/event/events/{event_id}"),
        Some(token),
        Some(&json!({ "status": "open" })),
    )
    .await
    .expect(StatusCode::OK);
    event_id
}

/// 发送验证码并返回 devCode。
async fn send_code(app: &TestApp, slug: &str, email: &str) -> String {
    let response = request(
        &app.app,
        "POST",
        &format!("/api/v1/event/public/events/{slug}/send-code"),
        None,
        Some(&json!({ "email": email })),
    )
    .await;
    response.expect(StatusCode::OK)["devCode"]
        .as_str()
        .expect("devCode")
        .to_string()
}

#[tokio::test]
async fn full_registration_lifecycle() {
    let app = spawn().await;
    let owner = Uuid::now_v7();
    let token = issue_token(&app, owner);
    let event_id = open_event(&app, &token, "welcome-2026", 2, false).await;

    // 公开信息含表单
    let public = request(
        &app.app,
        "GET",
        "/api/v1/event/public/events/welcome-2026",
        None,
        None,
    )
    .await;
    let public = public.expect(StatusCode::OK);
    assert_eq!(public["title"], "迎新活动");
    assert_eq!(public["remaining"], 2);
    assert!(public["form"]["fields"].as_array().unwrap().len() >= 3);

    // 错误验证码 → 400
    let code_bad = send_code(&app, "welcome-2026", "a@club.example.com").await;
    let wrong = request(
        &app.app,
        "POST",
        "/api/v1/event/public/events/welcome-2026/registrations",
        None,
        Some(&json!({
            "code": if code_bad == "000000" { "111111" } else { "000000" },
            "answers": { "name": "张三", "email": "a@club.example.com", "phone": "13800000000" }
        })),
    )
    .await;
    wrong.expect(StatusCode::BAD_REQUEST);

    // 正确验证码 → 通过
    let code = send_code(&app, "welcome-2026", "a@club.example.com").await;
    let first = request(
        &app.app,
        "POST",
        "/api/v1/event/public/events/welcome-2026/registrations",
        None,
        Some(&json!({
            "code": code,
            "answers": { "name": "张三", "email": "a@club.example.com", "phone": "13800000000" }
        })),
    )
    .await;
    let first = first.expect(StatusCode::CREATED);
    assert_eq!(first["status"], "approved");
    let checkin_code = first["checkinCode"].as_str().unwrap().to_string();

    // 重复报名 → 409
    let code2 = send_code(&app, "welcome-2026", "a@club.example.com").await;
    let dup = request(
        &app.app,
        "POST",
        "/api/v1/event/public/events/welcome-2026/registrations",
        None,
        Some(&json!({
            "code": code2,
            "answers": { "name": "张三", "email": "a@club.example.com" }
        })),
    )
    .await;
    dup.expect(StatusCode::CONFLICT);

    // 第二个通过、第三个进入候补（容量 2）
    let code3 = send_code(&app, "welcome-2026", "b@club.example.com").await;
    request(
        &app.app,
        "POST",
        "/api/v1/event/public/events/welcome-2026/registrations",
        None,
        Some(
            &json!({ "code": code3, "answers": { "name": "李四", "email": "b@club.example.com" } }),
        ),
    )
    .await
    .expect(StatusCode::CREATED);
    let code4 = send_code(&app, "welcome-2026", "c@club.example.com").await;
    let waitlisted = request(
        &app.app,
        "POST",
        "/api/v1/event/public/events/welcome-2026/registrations",
        None,
        Some(
            &json!({ "code": code4, "answers": { "name": "王五", "email": "c@club.example.com" } }),
        ),
    )
    .await;
    assert_eq!(waitlisted.expect(StatusCode::CREATED)["status"], "waitlist");

    // 名单 + 统计
    let list = request(
        &app.app,
        "GET",
        &format!("/api/v1/event/events/{event_id}/registrations"),
        Some(&token),
        None,
    )
    .await;
    let list = list.expect(StatusCode::OK);
    assert_eq!(list.as_array().unwrap().len(), 3);
    let stats = request(
        &app.app,
        "GET",
        &format!("/api/v1/event/events/{event_id}/stats"),
        Some(&token),
        None,
    )
    .await;
    let stats = stats.expect(StatusCode::OK);
    assert_eq!(stats["approved"], 2);
    assert_eq!(stats["waitlist"], 1);

    // 取消第一个 → 候补递补
    let first_id = first["id"].as_str().unwrap().to_string();
    let cancelled = request(
        &app.app,
        "POST",
        &format!("/api/v1/event/events/{event_id}/registrations/{first_id}/review"),
        Some(&token),
        Some(&json!({ "action": "cancel" })),
    )
    .await;
    assert_eq!(cancelled.expect(StatusCode::OK)["status"], "cancelled");
    let promoted = request(
        &app.app,
        "GET",
        &format!("/api/v1/event/events/{event_id}/registrations?status=approved"),
        Some(&token),
        None,
    )
    .await;
    let promoted = promoted.expect(StatusCode::OK);
    let emails: Vec<&str> = promoted
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["email"].as_str().unwrap())
        .collect();
    assert!(
        emails.contains(&"c@club.example.com"),
        "候补应递补：{emails:?}"
    );

    // 签到：已通过的第二个报名
    let second = promoted
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["email"] == "b@club.example.com")
        .unwrap()
        .clone();
    let second_id = second["id"].as_str().unwrap();
    let checked = request(
        &app.app,
        "POST",
        &format!("/api/v1/event/events/{event_id}/registrations/{second_id}/checkin"),
        Some(&token),
        None,
    )
    .await;
    assert!(!checked.expect(StatusCode::OK)["checkedInAt"].is_null());

    // 公开状态查询（签到码 + 邮箱）
    let status = request(
        &app.app,
        "GET",
        &format!("/api/v1/event/public/registrations/{checkin_code}?email=a%40club.example.com"),
        None,
        None,
    )
    .await;
    let status = status.expect(StatusCode::OK);
    assert_eq!(status["status"], "cancelled");
    // 邮箱不匹配 → 404
    let mismatch = request(
        &app.app,
        "GET",
        &format!("/api/v1/event/public/registrations/{checkin_code}?email=x%40club.example.com"),
        None,
        None,
    )
    .await;
    mismatch.expect(StatusCode::NOT_FOUND);

    // 导出 Excel
    let export = request(
        &app.app,
        "GET",
        &format!("/api/v1/event/events/{event_id}/export.xlsx"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(export.status, StatusCode::OK);
    assert_eq!(&export.bytes[..2], b"PK", "xlsx 应为 zip 容器");
}

#[tokio::test]
async fn review_email_domain_and_toggles() {
    let app = spawn().await;
    let owner = Uuid::now_v7();
    let token = issue_token(&app, owner);
    // 需要审核的事件
    let event_id = open_event(&app, &token, "review-me", 0, true).await;

    // 域名白名单（只允许 club.example.com）
    request(
        &app.app,
        "PATCH",
        &format!("/api/v1/event/events/{event_id}"),
        Some(&token),
        Some(&json!({ "emailDomains": ["club.example.com"] })),
    )
    .await
    .expect(StatusCode::OK);
    let blocked = request(
        &app.app,
        "POST",
        "/api/v1/event/public/events/review-me/send-code",
        None,
        Some(&json!({ "email": "x@evil.com" })),
    )
    .await;
    let blocked = blocked.expect(StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(blocked["code"], "EVENT_EMAIL_DOMAIN_NOT_ALLOWED");

    // 白名单内 → pending → 审核通过
    let code = send_code(&app, "review-me", "d@club.example.com").await;
    let created = request(
        &app.app,
        "POST",
        "/api/v1/event/public/events/review-me/registrations",
        None,
        Some(
            &json!({ "code": code, "answers": { "name": "赵六", "email": "d@club.example.com" } }),
        ),
    )
    .await;
    let created = created.expect(StatusCode::CREATED);
    assert_eq!(created["status"], "pending");
    let registration_id = created["id"].as_str().unwrap().to_string();
    let approved = request(
        &app.app,
        "POST",
        &format!("/api/v1/event/events/{event_id}/registrations/{registration_id}/review"),
        Some(&token),
        Some(&json!({ "action": "approve" })),
    )
    .await;
    assert_eq!(approved.expect(StatusCode::OK)["status"], "approved");
    // 未通过报名不可签到（用拒绝的来测）
    let rejected = request(
        &app.app,
        "POST",
        &format!("/api/v1/event/events/{event_id}/registrations/{registration_id}/review"),
        Some(&token),
        Some(&json!({ "action": "reject" })),
    )
    .await;
    rejected.expect(StatusCode::OK);
    let checkin = request(
        &app.app,
        "POST",
        &format!("/api/v1/event/events/{event_id}/registrations/{registration_id}/checkin"),
        Some(&token),
        None,
    )
    .await;
    checkin.expect(StatusCode::CONFLICT);

    // 关闭邮箱验证的事件：无验证码直接报名
    let open_id = open_event(&app, &token, "no-verify", 0, false).await;
    request(
        &app.app,
        "PATCH",
        &format!("/api/v1/event/events/{open_id}"),
        Some(&token),
        Some(&json!({ "emailVerify": false })),
    )
    .await
    .expect(StatusCode::OK);
    let direct = request(
        &app.app,
        "POST",
        "/api/v1/event/public/events/no-verify/registrations",
        None,
        Some(&json!({ "answers": { "name": "钱七", "email": "e@club.example.com" } })),
    )
    .await;
    assert_eq!(direct.expect(StatusCode::CREATED)["status"], "approved");
}

#[tokio::test]
async fn permissions_form_schema_and_validation() {
    let app = spawn().await;
    let owner = Uuid::now_v7();
    let outsider = Uuid::now_v7();
    let token = issue_token(&app, owner);
    let outsider_token = issue_token(&app, outsider);
    let event_id = open_event(&app, &token, "perm-test", 0, false).await;

    // 非管理员 403
    let denied = request(
        &app.app,
        "GET",
        &format!("/api/v1/event/events/{event_id}/registrations"),
        Some(&outsider_token),
        None,
    )
    .await;
    denied.expect(StatusCode::FORBIDDEN);

    // 非法表单 schema → 422
    let bad_form = request(
        &app.app,
        "PUT",
        &format!("/api/v1/event/events/{event_id}/form"),
        Some(&token),
        Some(&json!({ "fields": [{ "key": "x", "type": "select", "label": "X" }] })),
    )
    .await;
    bad_form.expect(StatusCode::UNPROCESSABLE_ENTITY);

    // 合法表单保存并读取
    let form = json!({
        "fields": [
            { "key": "name", "type": "text", "label": "姓名", "required": true },
            { "key": "email", "type": "email", "label": "邮箱", "required": true },
            { "key": "diet", "type": "select", "label": "饮食", "required": false,
              "options": [ { "label": "无", "value": "none" }, { "label": "素食", "value": "veg" } ] }
        ]
    });
    let saved = request(
        &app.app,
        "PUT",
        &format!("/api/v1/event/events/{event_id}/form"),
        Some(&token),
        Some(&form),
    )
    .await;
    assert!(saved.expect(StatusCode::OK)["version"].as_i64().unwrap() >= 2);

    // 答案校验：缺少必填 → 422
    let missing = request(
        &app.app,
        "POST",
        "/api/v1/event/public/events/perm-test/registrations",
        None,
        Some(&json!({ "answers": { "email": "f@club.example.com" } })),
    )
    .await;
    missing.expect(StatusCode::UNPROCESSABLE_ENTITY);

    // 非法 slug / 重复 slug / 非法状态
    let bad_slug = request(
        &app.app,
        "POST",
        "/api/v1/event/events",
        Some(&token),
        Some(&json!({ "slug": "Bad Slug", "title": "x" })),
    )
    .await;
    bad_slug.expect(StatusCode::UNPROCESSABLE_ENTITY);
    let dup_slug = request(
        &app.app,
        "POST",
        "/api/v1/event/events",
        Some(&token),
        Some(&json!({ "slug": "perm-test", "title": "x" })),
    )
    .await;
    dup_slug.expect(StatusCode::CONFLICT);
    let bad_status = request(
        &app.app,
        "PATCH",
        &format!("/api/v1/event/events/{event_id}"),
        Some(&token),
        Some(&json!({ "status": "closed-forever" })),
    )
    .await;
    bad_status.expect(StatusCode::UNPROCESSABLE_ENTITY);

    // 未登录 401 与健康检查
    request(&app.app, "GET", "/api/v1/event/events", None, None)
        .await
        .expect(StatusCode::UNAUTHORIZED);
    let health = request(&app.app, "GET", "/healthz", None, None).await;
    assert_eq!(health.expect(StatusCode::OK)["status"], "ok");
    let ready = request(&app.app, "GET", "/readyz", None, None).await;
    assert_eq!(ready.expect(StatusCode::OK)["database"], "ok");
}

#[tokio::test]
async fn config_and_schema_validation_paths() {
    let err = event_service::db::connect_with_schema(&test_database_url(), "Bad-Name").await;
    assert!(err.is_err());
    assert!(event_service::config::Config::from_map(Default::default()).is_err());
    std::env::set_var("DATABASE_URL", test_database_url());
    let config = event_service::config::Config::from_env().expect("env config");
    assert_eq!(config.bind_addr, "0.0.0.0:8086");
    std::env::remove_var("DATABASE_URL");
}
