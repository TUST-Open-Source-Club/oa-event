//! HTTP 路由。

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Duration, FixedOffset, Utc};
use rust_xlsxwriter::Workbook;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use club_auth_sdk::AuthUser;
use club_common::{validate, AppError, FieldError};

use crate::domain;
use crate::entity::{event, registration, verification_code};
use crate::repo;
use crate::state::SharedState;

/// 存活检查。
pub async fn healthz() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

/// 就绪检查。
pub async fn readyz(State(state): State<SharedState>) -> Json<Value> {
    match state.db.ping().await {
        Ok(_) => Json(json!({ "status": "ready", "database": "ok" })),
        Err(err) => {
            tracing::error!(error = %err, "数据库就绪检查失败");
            Json(json!({ "status": "degraded", "database": "error" }))
        }
    }
}

/// 登录用户 ID。
fn user_id_of(auth: &AuthUser) -> Result<Uuid, AppError> {
    auth.claims()
        .sub
        .parse()
        .map_err(|_| AppError::unauthorized("AUTH_INVALID_TOKEN", "访问令牌无效"))
}

/// 可选登录用户 ID。
fn optional_user_id(auth: &club_auth_sdk::OptionalAuthUser) -> Option<Uuid> {
    auth.0.as_ref().and_then(|claims| claims.sub.parse().ok())
}

/// 写入 outbox（notify）。
#[allow(clippy::too_many_arguments)]
async fn enqueue_event(
    state: &SharedState,
    event_type: &str,
    targets: Vec<Uuid>,
    event_id: Uuid,
    title: &str,
    body: &str,
) {
    if state.bus.is_none() || targets.is_empty() {
        return;
    }
    let payload = json!({
        "id": Uuid::now_v7(),
        "type": event_type,
        "targetUsers": targets.iter().map(|id| id.to_string()).collect::<Vec<_>>(),
        "resource": { "type": "event", "id": event_id, "url": format!("/events/{event_id}") },
        "title": title,
        "body": body,
        "priority": "high"
    });
    if let Err(err) = club_bus::outbox::enqueue(&state.db, event_type, &payload, Utc::now()).await {
        tracing::warn!(error = %err, "活动事件写入 outbox 失败");
    }
}

/// 活动 DTO。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventDto {
    /// ID。
    pub id: String,
    /// slug。
    pub slug: String,
    /// 标题。
    pub title: String,
    /// 详情。
    pub description_md: String,
    /// 地点。
    pub location: Option<String>,
    /// 名额（0 = 不限）。
    pub capacity: i64,
    /// 候补开关。
    pub waitlist_enabled: bool,
    /// 审核开关。
    pub need_review: bool,
    /// 邮箱验证开关。
    pub email_verify: bool,
    /// 域名白名单。
    pub email_domains: Vec<String>,
    /// 状态。
    pub status: String,
    /// 开始时间。
    pub start_at: Option<DateTime<FixedOffset>>,
    /// 结束时间。
    pub end_at: Option<DateTime<FixedOffset>>,
    /// 报名开始时间。
    pub reg_start_at: Option<DateTime<FixedOffset>>,
    /// 报名截止时间。
    pub reg_end_at: Option<DateTime<FixedOffset>>,
}

/// 从实体构造 DTO。
fn event_dto(model: &event::Model) -> EventDto {
    let domains = match &model.email_domains {
        Value::Array(items) => items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    };
    EventDto {
        id: model.id.to_string(),
        slug: model.slug.clone(),
        title: model.title.clone(),
        description_md: model.description_md.clone(),
        location: model.location.clone(),
        capacity: model.capacity,
        waitlist_enabled: model.waitlist_enabled,
        need_review: model.need_review,
        email_verify: model.email_verify,
        email_domains: domains,
        status: model.status.clone(),
        start_at: model.start_at,
        end_at: model.end_at,
        reg_start_at: model.reg_start_at,
        reg_end_at: model.reg_end_at,
    }
}

/// 报名 DTO。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistrationDto {
    /// ID。
    pub id: String,
    /// 姓名。
    pub name: Option<String>,
    /// 邮箱。
    pub email: String,
    /// 手机。
    pub phone: Option<String>,
    /// 答案。
    pub answers: Value,
    /// 状态。
    pub status: String,
    /// 签到码。
    pub checkin_code: String,
    /// 签到时间。
    pub checked_in_at: Option<DateTime<FixedOffset>>,
    /// 报名时间。
    pub created_at: DateTime<FixedOffset>,
}

/// 从实体构造报名 DTO。
fn registration_dto(model: &registration::Model) -> RegistrationDto {
    RegistrationDto {
        id: model.id.to_string(),
        name: model.name.clone(),
        email: model.email.clone(),
        phone: model.phone.clone(),
        answers: model.answers.clone(),
        status: model.status.clone(),
        checkin_code: model.checkin_code.clone(),
        checked_in_at: model.checked_in_at,
        created_at: model.created_at,
    }
}

/// 创建活动请求。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateEventRequest {
    /// slug（小写字母数字与 -）。
    pub slug: String,
    /// 标题。
    pub title: String,
    /// 详情。
    pub description_md: Option<String>,
    /// 名额（0 = 不限）。
    pub capacity: Option<i64>,
    /// 候补开关。
    pub waitlist_enabled: Option<bool>,
    /// 审核开关。
    pub need_review: Option<bool>,
    /// 邮箱验证开关。
    pub email_verify: Option<bool>,
}

/// 校验 slug。
fn validate_slug(slug: &str) -> Result<(), AppError> {
    if slug.is_empty()
        || slug.len() > 64
        || !slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(AppError::unprocessable(
            "EVENT_VALIDATION",
            "slug 仅支持小写字母/数字/连字符",
            vec![FieldError::new("slug", "非法")],
        ));
    }
    Ok(())
}

/// `POST /events`。
pub async fn create_event(
    State(state): State<SharedState>,
    auth: AuthUser,
    Json(input): Json<CreateEventRequest>,
) -> Result<(StatusCode, Json<EventDto>), AppError> {
    let user_id = user_id_of(&auth)?;
    validate_slug(&input.slug)?;
    let title = input.title.trim();
    if title.is_empty() || title.chars().count() > 200 {
        return Err(AppError::unprocessable(
            "EVENT_VALIDATION",
            "标题需为 1 ~ 200 字符",
            vec![FieldError::new("title", "非法")],
        ));
    }
    if repo::find_event_by_slug(&state.db, &input.slug)
        .await?
        .is_some()
    {
        return Err(AppError::conflict("EVENT_SLUG_TAKEN", "slug 已被使用"));
    }
    let capacity = input.capacity.unwrap_or(0);
    if capacity < 0 {
        return Err(AppError::unprocessable(
            "EVENT_VALIDATION",
            "名额不能为负",
            vec![FieldError::new("capacity", "非法")],
        ));
    }
    let model = repo::create_event(
        &state.db,
        &input.slug,
        title,
        input.description_md.as_deref().unwrap_or(""),
        capacity,
        input.waitlist_enabled.unwrap_or(true),
        input.need_review.unwrap_or(false),
        input.email_verify.unwrap_or(true),
        user_id,
        state.now(),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(event_dto(&model))))
}

/// `GET /events`。
pub async fn list_events(
    State(state): State<SharedState>,
    auth: AuthUser,
) -> Result<Json<Vec<EventDto>>, AppError> {
    let user_id = user_id_of(&auth)?;
    let events = repo::list_events(&state.db, user_id).await?;
    Ok(Json(events.iter().map(event_dto).collect()))
}

/// 加载活动并校验管理员。
async fn load_event_for_admin(
    state: &SharedState,
    user_id: Uuid,
    event_id: Uuid,
) -> Result<event::Model, AppError> {
    let model = repo::find_event(&state.db, event_id)
        .await?
        .ok_or_else(|| AppError::not_found("EVENT_NOT_FOUND", "活动不存在"))?;
    repo::ensure_event_admin(&state.db, event_id, user_id).await?;
    Ok(model)
}

/// `GET /events/{id}`。
pub async fn get_event(
    State(state): State<SharedState>,
    auth: AuthUser,
    Path(event_id): Path<Uuid>,
) -> Result<Json<EventDto>, AppError> {
    let user_id = user_id_of(&auth)?;
    let model = load_event_for_admin(&state, user_id, event_id).await?;
    Ok(Json(event_dto(&model)))
}

/// 更新活动请求。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateEventRequest {
    /// 标题。
    pub title: Option<String>,
    /// 详情。
    pub description_md: Option<String>,
    /// 地点（null 清空）。
    #[serde(default, deserialize_with = "double_option")]
    pub location: Option<Option<String>>,
    /// 名额。
    pub capacity: Option<i64>,
    /// 候补开关。
    pub waitlist_enabled: Option<bool>,
    /// 审核开关。
    pub need_review: Option<bool>,
    /// 邮箱验证开关。
    pub email_verify: Option<bool>,
    /// 域名白名单。
    pub email_domains: Option<Vec<String>>,
    /// draft / open / closed。
    pub status: Option<String>,
}

/// 三态 JSON：缺省 None，null = Some(None)，值 = Some(Some(v))。
fn double_option<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

/// `PATCH /events/{id}`。
pub async fn update_event(
    State(state): State<SharedState>,
    auth: AuthUser,
    Path(event_id): Path<Uuid>,
    Json(input): Json<UpdateEventRequest>,
) -> Result<Json<EventDto>, AppError> {
    let user_id = user_id_of(&auth)?;
    let model = load_event_for_admin(&state, user_id, event_id).await?;
    if let Some(status) = input.status.as_deref() {
        if ![
            event::STATUS_DRAFT,
            event::STATUS_OPEN,
            event::STATUS_CLOSED,
        ]
        .contains(&status)
        {
            return Err(AppError::unprocessable(
                "EVENT_VALIDATION",
                "状态不合法",
                vec![FieldError::new("status", "仅支持 draft/open/closed")],
            ));
        }
    }
    if let Some(capacity) = input.capacity {
        if capacity < 0 {
            return Err(AppError::unprocessable(
                "EVENT_VALIDATION",
                "名额不能为负",
                vec![FieldError::new("capacity", "非法")],
            ));
        }
    }
    let updated = repo::update_event(
        &state.db,
        &model,
        input.title,
        input.description_md,
        input.location,
        input.capacity,
        input.waitlist_enabled,
        input.need_review,
        input.email_verify,
        input.email_domains,
        input.status,
        state.now(),
    )
    .await?;
    Ok(Json(event_dto(&updated)))
}

/// `PUT /events/{id}/form`：保存表单 schema。
pub async fn put_form(
    State(state): State<SharedState>,
    auth: AuthUser,
    Path(event_id): Path<Uuid>,
    Json(schema): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let user_id = user_id_of(&auth)?;
    load_event_for_admin(&state, user_id, event_id).await?;
    domain::validate_form_schema(&schema)?;
    let model = repo::save_form_schema(&state.db, event_id, &schema, state.now()).await?;
    Ok(Json(
        json!({ "version": model.version, "schema": model.schema }),
    ))
}

/// `GET /events/{id}/form`。
pub async fn get_form(
    State(state): State<SharedState>,
    auth: AuthUser,
    Path(event_id): Path<Uuid>,
) -> Result<Json<Value>, AppError> {
    let user_id = user_id_of(&auth)?;
    load_event_for_admin(&state, user_id, event_id).await?;
    let model = repo::latest_form_schema(&state.db, event_id)
        .await?
        .ok_or_else(|| AppError::not_found("EVENT_FORM_NOT_FOUND", "表单不存在"))?;
    Ok(Json(
        json!({ "version": model.version, "schema": model.schema }),
    ))
}

/// 名单查询。
#[derive(Debug, Deserialize)]
pub struct RegistrationQuery {
    /// 状态过滤。
    pub status: Option<String>,
}

/// `GET /events/{id}/registrations`。
pub async fn list_registrations(
    State(state): State<SharedState>,
    auth: AuthUser,
    Path(event_id): Path<Uuid>,
    Query(query): Query<RegistrationQuery>,
) -> Result<Json<Vec<RegistrationDto>>, AppError> {
    let user_id = user_id_of(&auth)?;
    load_event_for_admin(&state, user_id, event_id).await?;
    let items = repo::list_registrations(&state.db, event_id, query.status.as_deref()).await?;
    Ok(Json(items.iter().map(registration_dto).collect()))
}

/// 审核/取消报名请求。
#[derive(Debug, Deserialize)]
pub struct ReviewRequest {
    /// approve / reject / cancel。
    pub action: String,
}

/// `POST /events/{id}/registrations/{rid}/review`。
pub async fn review_registration(
    State(state): State<SharedState>,
    auth: AuthUser,
    Path((event_id, registration_id)): Path<(Uuid, Uuid)>,
    Json(input): Json<ReviewRequest>,
) -> Result<Json<RegistrationDto>, AppError> {
    let user_id = user_id_of(&auth)?;
    let event_model = load_event_for_admin(&state, user_id, event_id).await?;
    let model = repo::find_registration(&state.db, registration_id)
        .await?
        .filter(|item| item.event_id == event_id)
        .ok_or_else(|| AppError::not_found("EVENT_REGISTRATION_NOT_FOUND", "报名不存在"))?;
    match input.action.as_str() {
        "approve" => {
            let counts = repo::status_counts(&state.db, event_id).await?;
            if event_model.capacity > 0 && counts.approved >= event_model.capacity {
                return Err(AppError::conflict("EVENT_FULL", "名额已满"));
            }
            let updated =
                repo::set_registration_status(&state.db, &model, domain::STATUS_APPROVED).await?;
            if let Some(target) = updated.user_id {
                enqueue_event(
                    &state,
                    "event.registration.approved",
                    vec![target],
                    event_id,
                    "报名已通过",
                    &event_model.title,
                )
                .await;
            }
            Ok(Json(registration_dto(&updated)))
        }
        "reject" | "cancel" => {
            let status = if input.action == "reject" {
                domain::STATUS_REJECTED
            } else {
                domain::STATUS_CANCELLED
            };
            let updated = repo::set_registration_status(&state.db, &model, status).await?;
            // 释放名额时递补最早的候补
            if model.status == domain::STATUS_APPROVED {
                if let Some(next) = repo::earliest_waitlist(&state.db, event_id).await? {
                    let promoted =
                        repo::set_registration_status(&state.db, &next, domain::STATUS_APPROVED)
                            .await?;
                    if let Some(target) = promoted.user_id {
                        enqueue_event(
                            &state,
                            "event.registration.approved",
                            vec![target],
                            event_id,
                            "候补递补成功",
                            &event_model.title,
                        )
                        .await;
                    }
                }
            }
            Ok(Json(registration_dto(&updated)))
        }
        _ => Err(AppError::unprocessable(
            "EVENT_VALIDATION",
            "action 不合法",
            vec![FieldError::new("action", "仅支持 approve/reject/cancel")],
        )),
    }
}

/// `POST /events/{id}/registrations/{rid}/checkin`：现场签到。
pub async fn checkin_registration(
    State(state): State<SharedState>,
    auth: AuthUser,
    Path((event_id, registration_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<RegistrationDto>, AppError> {
    let user_id = user_id_of(&auth)?;
    load_event_for_admin(&state, user_id, event_id).await?;
    let model = repo::find_registration(&state.db, registration_id)
        .await?
        .filter(|item| item.event_id == event_id)
        .ok_or_else(|| AppError::not_found("EVENT_REGISTRATION_NOT_FOUND", "报名不存在"))?;
    if model.status != domain::STATUS_APPROVED {
        return Err(AppError::conflict(
            "EVENT_NOT_APPROVED",
            "仅已通过的报名可签到",
        ));
    }
    if model.checked_in_at.is_some() {
        return Ok(Json(registration_dto(&model)));
    }
    let updated = repo::set_checked_in(&state.db, &model, state.now()).await?;
    Ok(Json(registration_dto(&updated)))
}

/// `GET /events/{id}/stats`。
pub async fn stats(
    State(state): State<SharedState>,
    auth: AuthUser,
    Path(event_id): Path<Uuid>,
) -> Result<Json<Value>, AppError> {
    let user_id = user_id_of(&auth)?;
    load_event_for_admin(&state, user_id, event_id).await?;
    let counts = repo::status_counts(&state.db, event_id).await?;
    Ok(Json(json!({
        "total": counts.total,
        "approved": counts.approved,
        "pending": counts.pending,
        "waitlist": counts.waitlist,
        "checkedIn": counts.checked_in
    })))
}

/// `GET /events/{id}/export.xlsx`：导出报名表。
pub async fn export_registrations(
    State(state): State<SharedState>,
    auth: AuthUser,
    Path(event_id): Path<Uuid>,
    Query(query): Query<RegistrationQuery>,
) -> Result<Response, AppError> {
    let user_id = user_id_of(&auth)?;
    let event_model = load_event_for_admin(&state, user_id, event_id).await?;
    let schema = repo::latest_form_schema(&state.db, event_id)
        .await?
        .map(|model| model.schema)
        .unwrap_or_else(domain::default_schema);
    let rows = repo::list_registrations(&state.db, event_id, query.status.as_deref()).await?;

    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    let headers = domain::export_headers(&schema);
    for (col, header) in headers.iter().enumerate() {
        sheet
            .write_string(0, col as u16, header)
            .map_err(AppError::internal)?;
    }
    // 自定义字段顺序（跳过内建联系字段）
    let fields: Vec<(String, String)> = schema
        .get("fields")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|field| {
                    let key = field.get("key")?.as_str()?.to_string();
                    if matches!(key.as_str(), "name" | "email" | "phone") {
                        return None;
                    }
                    let label = field.get("label")?.as_str()?.to_string();
                    Some((key, label))
                })
                .collect()
        })
        .unwrap_or_default();

    for (index, row) in rows.iter().enumerate() {
        let line = index as u32 + 1;
        sheet
            .write_number(line, 0, line)
            .map_err(AppError::internal)?;
        sheet
            .write_string(line, 1, row.name.as_deref().unwrap_or(""))
            .map_err(AppError::internal)?;
        sheet
            .write_string(line, 2, &row.email)
            .map_err(AppError::internal)?;
        sheet
            .write_string(line, 3, row.phone.as_deref().unwrap_or(""))
            .map_err(AppError::internal)?;
        sheet
            .write_string(line, 4, domain::status_label(&row.status))
            .map_err(AppError::internal)?;
        sheet
            .write_string(
                line,
                5,
                if row.checked_in_at.is_some() {
                    "是"
                } else {
                    "否"
                },
            )
            .map_err(AppError::internal)?;
        sheet
            .write_string(line, 6, row.created_at.to_rfc3339())
            .map_err(AppError::internal)?;
        for (offset, (key, _)) in fields.iter().enumerate() {
            let text = row
                .answers
                .get(key)
                .map(|value| match value {
                    Value::String(text) => text.clone(),
                    Value::Array(items) => items
                        .iter()
                        .map(|item| item.as_str().unwrap_or("").to_string())
                        .collect::<Vec<_>>()
                        .join(", "),
                    Value::Null => String::new(),
                    other => other.to_string(),
                })
                .unwrap_or_default();
            sheet
                .write_string(line, 7 + offset as u16, text)
                .map_err(AppError::internal)?;
        }
    }
    let buffer = workbook.save_to_buffer().map_err(AppError::internal)?;
    let filename = percent_encode_header(&format!("{}_报名表.xlsx", event_model.title));
    let mut response = buffer.into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        ),
    );
    if let Ok(value) = HeaderValue::from_str(&format!("attachment; filename*=UTF-8''{}", filename))
    {
        response
            .headers_mut()
            .insert(header::CONTENT_DISPOSITION, value);
    }
    Ok(response)
}

/// 百分号编码（RFC 5987）。
fn percent_encode_header(name: &str) -> String {
    name.bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'~') {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

/// `GET /public/events/{slug}`：公开活动信息与表单。
pub async fn public_event(
    State(state): State<SharedState>,
    Path(slug): Path<String>,
) -> Result<Json<Value>, AppError> {
    let model = repo::find_event_by_slug(&state.db, &slug)
        .await?
        .filter(|model| model.status == event::STATUS_OPEN)
        .ok_or_else(|| AppError::not_found("EVENT_NOT_FOUND", "活动不存在或未开放"))?;
    let schema = repo::latest_form_schema(&state.db, model.id)
        .await?
        .map(|form| form.schema)
        .unwrap_or_else(domain::default_schema);
    let counts = repo::status_counts(&state.db, model.id).await?;
    let remaining = if model.capacity > 0 {
        Some((model.capacity - counts.approved).max(0))
    } else {
        None
    };
    Ok(Json(json!({
        "id": model.id,
        "slug": model.slug,
        "title": model.title,
        "descriptionMd": model.description_md,
        "location": model.location,
        "capacity": model.capacity,
        "remaining": remaining,
        "waitlistEnabled": model.waitlist_enabled,
        "needReview": model.need_review,
        "emailVerify": model.email_verify,
        "form": schema
    })))
}

/// 发送验证码请求。
#[derive(Debug, Deserialize)]
pub struct SendCodeRequest {
    /// 邮箱。
    pub email: String,
}

/// 验证码有效期（秒）。
const CODE_TTL_SECONDS: i64 = 300;

/// `POST /public/events/{slug}/send-code`。
pub async fn send_code(
    State(state): State<SharedState>,
    Path(slug): Path<String>,
    Json(input): Json<SendCodeRequest>,
) -> Result<Json<Value>, AppError> {
    let model = repo::find_event_by_slug(&state.db, &slug)
        .await?
        .filter(|model| model.status == event::STATUS_OPEN)
        .ok_or_else(|| AppError::not_found("EVENT_NOT_FOUND", "活动不存在或未开放"))?;
    if !model.email_verify {
        return Err(AppError::bad_request(
            "EVENT_VERIFY_DISABLED",
            "该活动无需邮箱验证",
        ));
    }
    let email = validate::normalize_email(&input.email);
    if !validate::is_valid_email(&email) {
        return Err(AppError::unprocessable(
            "EVENT_VALIDATION",
            "邮箱格式错误",
            vec![FieldError::new("email", "格式错误")],
        ));
    }
    let domains: Vec<String> = match &model.email_domains {
        Value::Array(items) => items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    };
    if !domain::email_allowed(&email, &domains) {
        return Err(AppError::unprocessable(
            "EVENT_EMAIL_DOMAIN_NOT_ALLOWED",
            "该邮箱域名不允许报名",
            vec![FieldError::new("email", "域名不在白名单")],
        ));
    }
    let code = domain::generate_code();
    let now = state.now();
    repo::insert_verification_code(
        &state.db,
        model.id,
        &email,
        verification_code::PURPOSE_REGISTER,
        &domain::hash_code(&code),
        now + Duration::seconds(CODE_TTL_SECONDS),
        now,
    )
    .await?;
    // 邮件服务接入前：开发模式返回验证码，生产仅记录日志
    tracing::info!(email = %email, event = %model.slug, "报名验证码已生成");
    let dev_code = state.config.dev_mode.then_some(code);
    Ok(Json(json!({ "sent": true, "devCode": dev_code })))
}

/// 报名请求。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterRequest {
    /// 邮箱验证码（开启验证时必填）。
    pub code: Option<String>,
    /// 表单答案。
    pub answers: Value,
}

/// `POST /public/events/{slug}/registrations`：游客/成员报名。
pub async fn register(
    State(state): State<SharedState>,
    auth: club_auth_sdk::OptionalAuthUser,
    Path(slug): Path<String>,
    Json(input): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    let model = repo::find_event_by_slug(&state.db, &slug)
        .await?
        .filter(|model| model.status == event::STATUS_OPEN)
        .ok_or_else(|| AppError::not_found("EVENT_NOT_FOUND", "活动不存在或未开放"))?;
    let now = state.now();
    // 报名窗口
    if let Some(start) = model.reg_start_at {
        if now.fixed_offset() < start {
            return Err(AppError::forbidden("EVENT_REG_NOT_STARTED", "报名尚未开始"));
        }
    }
    if let Some(end) = model.reg_end_at {
        if now.fixed_offset() > end {
            return Err(AppError::forbidden("EVENT_REG_CLOSED", "报名已截止"));
        }
    }
    let form = repo::latest_form_schema(&state.db, model.id)
        .await?
        .ok_or_else(|| AppError::internal("活动缺少表单"))?;
    domain::validate_answers(&form.schema, &input.answers)?;

    let email = input
        .answers
        .get("email")
        .and_then(Value::as_str)
        .map(validate::normalize_email)
        .ok_or_else(|| {
            AppError::unprocessable(
                "EVENT_VALIDATION",
                "缺少邮箱",
                vec![FieldError::new("email", "必填")],
            )
        })?;
    if !validate::is_valid_email(&email) {
        return Err(AppError::unprocessable(
            "EVENT_VALIDATION",
            "邮箱格式错误",
            vec![FieldError::new("email", "格式错误")],
        ));
    }
    let domains: Vec<String> = match &model.email_domains {
        Value::Array(items) => items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    };
    if !domain::email_allowed(&email, &domains) {
        return Err(AppError::unprocessable(
            "EVENT_EMAIL_DOMAIN_NOT_ALLOWED",
            "该邮箱域名不允许报名",
            vec![FieldError::new("email", "域名不在白名单")],
        ));
    }
    if repo::find_active_registration(&state.db, model.id, &email)
        .await?
        .is_some()
    {
        return Err(AppError::conflict(
            "EVENT_ALREADY_REGISTERED",
            "该邮箱已报名",
        ));
    }

    // 邮箱验证码
    if model.email_verify {
        let code = input.code.as_deref().map(str::trim).unwrap_or("");
        let Some(record) = repo::latest_verification_code(
            &state.db,
            model.id,
            &email,
            verification_code::PURPOSE_REGISTER,
        )
        .await?
        else {
            return Err(AppError::bad_request(
                "EVENT_CODE_INVALID",
                "请先获取验证码",
            ));
        };
        if record.expires_at < now.fixed_offset() {
            return Err(AppError::bad_request("EVENT_CODE_EXPIRED", "验证码已过期"));
        }
        if record.attempts >= 5 {
            return Err(AppError::too_many_requests(
                "EVENT_CODE_LOCKED",
                "验证码尝试次数过多，请重新获取",
            ));
        }
        if domain::hash_code(code) != record.code_hash {
            repo::bump_code_attempts(&state.db, &record).await?;
            return Err(AppError::bad_request("EVENT_CODE_INVALID", "验证码错误"));
        }
        repo::mark_code_used(&state.db, &record, now).await?;
    }

    let counts = repo::status_counts(&state.db, model.id).await?;
    let decision = domain::registration_decision(
        counts.approved,
        model.capacity,
        model.waitlist_enabled,
        model.need_review,
    );
    // 满员且未启用候补 → 拒绝
    if model.capacity > 0
        && counts.approved >= model.capacity
        && decision == domain::Decision::Approved
    {
        return Err(AppError::conflict("EVENT_FULL", "名额已满"));
    }
    let status = match decision {
        domain::Decision::Approved => domain::STATUS_APPROVED,
        domain::Decision::Pending => domain::STATUS_PENDING,
        domain::Decision::Waitlist => domain::STATUS_WAITLIST,
    };
    let (name, phone) = domain::extract_contact(&input.answers);
    let registration = repo::create_registration(
        &state.db,
        model.id,
        form.version,
        optional_user_id(&auth),
        name,
        &email,
        phone,
        &input.answers,
        status,
        now,
    )
    .await?;
    enqueue_event(
        &state,
        "event.registration.created",
        vec![model.created_by],
        model.id,
        "有新报名",
        &model.title,
    )
    .await;
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "id": registration.id,
            "status": registration.status,
            "checkinCode": registration.checkin_code,
            "email": registration.email
        })),
    ))
}

/// 报名状态查询。
#[derive(Debug, Deserialize)]
pub struct StatusQuery {
    /// 报名邮箱（与签到码双因子校验）。
    pub email: String,
}

/// `GET /public/registrations/{code}`。
pub async fn registration_status(
    State(state): State<SharedState>,
    Path(code): Path<String>,
    Query(query): Query<StatusQuery>,
) -> Result<Json<Value>, AppError> {
    let email = validate::normalize_email(&query.email);
    let model = repo::find_registration_by_code(&state.db, &code)
        .await?
        .filter(|model| model.email == email)
        .ok_or_else(|| AppError::not_found("EVENT_REGISTRATION_NOT_FOUND", "报名不存在"))?;
    Ok(Json(json!({
        "status": model.status,
        "statusLabel": domain::status_label(&model.status),
        "checkedIn": model.checked_in_at.is_some(),
        "createdAt": model.created_at
    })))
}

/// `/api/v1/event` 路由。
pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/events", get(list_events).post(create_event))
        .route("/events/{id}", get(get_event).patch(update_event))
        .route("/events/{id}/form", get(get_form).put(put_form))
        .route("/events/{id}/registrations", get(list_registrations))
        .route(
            "/events/{id}/registrations/{rid}/review",
            post(review_registration),
        )
        .route(
            "/events/{id}/registrations/{rid}/checkin",
            post(checkin_registration),
        )
        .route("/events/{id}/stats", get(stats))
        .route("/events/{id}/export.xlsx", get(export_registrations))
        .route("/public/events/{slug}", get(public_event))
        .route("/public/events/{slug}/send-code", post(send_code))
        .route("/public/events/{slug}/registrations", post(register))
        .route("/public/registrations/{code}", get(registration_status))
}
