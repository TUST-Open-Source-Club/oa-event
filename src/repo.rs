//! 数据访问层：只操作 event schema。

use chrono::{DateTime, Utc};
use sea_orm::sea_query::{Expr, ExprTrait};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QueryOrder,
    Set,
};
use serde_json::Value;
use uuid::Uuid;

use club_common::{new_id, AppError};

use crate::domain;
use crate::entity::{event, event_admin, form_schema, registration, verification_code};

/// 数据库错误 → 统一错误。
pub fn map_db_err(err: DbErr) -> AppError {
    AppError::internal(err)
}

/// 状态计数。
#[derive(Debug, Clone, Copy, Default)]
pub struct StatusCounts {
    /// 已通过。
    pub approved: i64,
    /// 待审核（含候补）。
    pub pending: i64,
    /// 候补。
    pub waitlist: i64,
    /// 已签到。
    pub checked_in: i64,
    /// 总数。
    pub total: i64,
}

/// 创建活动（创建者为管理员，附默认表单）。
#[allow(clippy::too_many_arguments)]
pub async fn create_event(
    db: &DatabaseConnection,
    slug: &str,
    title: &str,
    description_md: &str,
    capacity: i64,
    waitlist_enabled: bool,
    need_review: bool,
    email_verify: bool,
    created_by: Uuid,
    now: DateTime<Utc>,
) -> Result<event::Model, AppError> {
    let model = event::ActiveModel {
        id: Set(new_id()),
        slug: Set(slug.to_string()),
        title: Set(title.to_string()),
        description_md: Set(description_md.to_string()),
        location: Set(None),
        start_at: Set(None),
        end_at: Set(None),
        reg_start_at: Set(None),
        reg_end_at: Set(None),
        capacity: Set(capacity),
        waitlist_enabled: Set(waitlist_enabled),
        need_review: Set(need_review),
        email_verify: Set(email_verify),
        email_domains: Set(serde_json::json!([])),
        status: Set(event::STATUS_DRAFT.to_string()),
        created_by: Set(created_by),
        created_at: Set(now.fixed_offset()),
        updated_at: Set(now.fixed_offset()),
    }
    .insert(db)
    .await
    .map_err(map_db_err)?;
    event_admin::ActiveModel {
        event_id: Set(model.id),
        user_id: Set(created_by),
        joined_at: Set(now.fixed_offset()),
    }
    .insert(db)
    .await
    .map_err(map_db_err)?;
    save_form_schema(db, model.id, &domain::default_schema(), now).await?;
    Ok(model)
}

/// 我管理的活动。
pub async fn list_events(
    db: &DatabaseConnection,
    user_id: Uuid,
) -> Result<Vec<event::Model>, AppError> {
    let admins = event_admin::Entity::find()
        .filter(event_admin::Column::UserId.eq(user_id))
        .all(db)
        .await
        .map_err(map_db_err)?;
    let ids: Vec<Uuid> = admins.iter().map(|a| a.event_id).collect();
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    event::Entity::find()
        .filter(event::Column::Id.is_in(ids))
        .order_by_desc(event::Column::CreatedAt)
        .all(db)
        .await
        .map_err(map_db_err)
}

/// 按 ID 查找活动。
pub async fn find_event(
    db: &DatabaseConnection,
    event_id: Uuid,
) -> Result<Option<event::Model>, AppError> {
    event::Entity::find_by_id(event_id)
        .one(db)
        .await
        .map_err(map_db_err)
}

/// 按 slug 查找活动。
pub async fn find_event_by_slug(
    db: &DatabaseConnection,
    slug: &str,
) -> Result<Option<event::Model>, AppError> {
    event::Entity::find()
        .filter(event::Column::Slug.eq(slug))
        .one(db)
        .await
        .map_err(map_db_err)
}

/// 校验活动管理员。
pub async fn ensure_event_admin(
    db: &DatabaseConnection,
    event_id: Uuid,
    user_id: Uuid,
) -> Result<(), AppError> {
    event_admin::Entity::find_by_id((event_id, user_id))
        .one(db)
        .await
        .map_err(map_db_err)?
        .map(|_| ())
        .ok_or_else(|| AppError::forbidden("EVENT_NOT_ADMIN", "无权管理该活动"))
}

/// 更新活动基础信息（None = 不修改）。
#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_arguments)]
pub async fn update_event(
    db: &DatabaseConnection,
    model: &event::Model,
    title: Option<String>,
    description_md: Option<String>,
    location: Option<Option<String>>,
    capacity: Option<i64>,
    waitlist_enabled: Option<bool>,
    need_review: Option<bool>,
    email_verify: Option<bool>,
    email_domains: Option<Vec<String>>,
    status: Option<String>,
    now: DateTime<Utc>,
) -> Result<event::Model, AppError> {
    let mut active: event::ActiveModel = model.clone().into();
    if let Some(title) = title {
        active.title = Set(title);
    }
    if let Some(description_md) = description_md {
        active.description_md = Set(description_md);
    }
    if let Some(location) = location {
        active.location = Set(location);
    }
    if let Some(capacity) = capacity {
        active.capacity = Set(capacity);
    }
    if let Some(waitlist_enabled) = waitlist_enabled {
        active.waitlist_enabled = Set(waitlist_enabled);
    }
    if let Some(need_review) = need_review {
        active.need_review = Set(need_review);
    }
    if let Some(email_verify) = email_verify {
        active.email_verify = Set(email_verify);
    }
    if let Some(domains) = email_domains {
        active.email_domains = Set(serde_json::json!(domains));
    }
    if let Some(status) = status {
        active.status = Set(status);
    }
    active.updated_at = Set(now.fixed_offset());
    active.update(db).await.map_err(map_db_err)
}

/// 保存表单 schema（自动递增版本）。
pub async fn save_form_schema(
    db: &DatabaseConnection,
    event_id: Uuid,
    schema: &Value,
    now: DateTime<Utc>,
) -> Result<form_schema::Model, AppError> {
    let max: Option<i64> = form_schema::Entity::find()
        .filter(form_schema::Column::EventId.eq(event_id))
        .order_by_desc(form_schema::Column::Version)
        .one(db)
        .await
        .map_err(map_db_err)?
        .map(|row| row.version);
    form_schema::ActiveModel {
        id: Set(new_id()),
        event_id: Set(event_id),
        version: Set(max.unwrap_or(0) + 1),
        schema: Set(schema.clone()),
        created_at: Set(now.fixed_offset()),
    }
    .insert(db)
    .await
    .map_err(map_db_err)
}

/// 最新表单 schema。
pub async fn latest_form_schema(
    db: &DatabaseConnection,
    event_id: Uuid,
) -> Result<Option<form_schema::Model>, AppError> {
    form_schema::Entity::find()
        .filter(form_schema::Column::EventId.eq(event_id))
        .order_by_desc(form_schema::Column::Version)
        .one(db)
        .await
        .map_err(map_db_err)
}

/// 状态计数。
pub async fn status_counts(
    db: &DatabaseConnection,
    event_id: Uuid,
) -> Result<StatusCounts, AppError> {
    let rows = registration::Entity::find()
        .filter(registration::Column::EventId.eq(event_id))
        .all(db)
        .await
        .map_err(map_db_err)?;
    let mut counts = StatusCounts::default();
    for row in rows {
        counts.total += 1;
        match row.status.as_str() {
            "approved" => counts.approved += 1,
            "pending" => counts.pending += 1,
            "waitlist" => counts.waitlist += 1,
            _ => {}
        }
        if row.checked_in_at.is_some() {
            counts.checked_in += 1;
        }
    }
    Ok(counts)
}

/// 是否存在有效报名（同活动同邮箱仅一条，候补视为有效）。
pub async fn find_active_registration(
    db: &DatabaseConnection,
    event_id: Uuid,
    email: &str,
) -> Result<Option<registration::Model>, AppError> {
    registration::Entity::find()
        .filter(registration::Column::EventId.eq(event_id))
        .filter(registration::Column::Email.eq(email))
        .filter(registration::Column::Status.is_in(["approved", "pending", "waitlist"]))
        .one(db)
        .await
        .map_err(map_db_err)
}

/// 创建报名。
pub async fn create_registration(
    db: &DatabaseConnection,
    event_id: Uuid,
    schema_version: i64,
    user_id: Option<Uuid>,
    name: Option<String>,
    email: &str,
    phone: Option<String>,
    answers: &Value,
    status: &str,
    now: DateTime<Utc>,
) -> Result<registration::Model, AppError> {
    registration::ActiveModel {
        id: Set(new_id()),
        event_id: Set(event_id),
        schema_version: Set(schema_version),
        user_id: Set(user_id),
        name: Set(name),
        email: Set(email.to_string()),
        phone: Set(phone),
        answers: Set(answers.clone()),
        status: Set(status.to_string()),
        checkin_code: Set(domain::generate_checkin_code()),
        checked_in_at: Set(None),
        created_at: Set(now.fixed_offset()),
    }
    .insert(db)
    .await
    .map_err(map_db_err)
}

/// 查找报名。
pub async fn find_registration(
    db: &DatabaseConnection,
    registration_id: Uuid,
) -> Result<Option<registration::Model>, AppError> {
    registration::Entity::find_by_id(registration_id)
        .one(db)
        .await
        .map_err(map_db_err)
}

/// 按签到码 + 邮箱查询（公开状态查询）。
pub async fn find_registration_by_code(
    db: &DatabaseConnection,
    checkin_code: &str,
) -> Result<Option<registration::Model>, AppError> {
    registration::Entity::find()
        .filter(registration::Column::CheckinCode.eq(checkin_code))
        .one(db)
        .await
        .map_err(map_db_err)
}

/// 报名名单（可过滤状态）。
pub async fn list_registrations(
    db: &DatabaseConnection,
    event_id: Uuid,
    status: Option<&str>,
) -> Result<Vec<registration::Model>, AppError> {
    let mut select =
        registration::Entity::find().filter(registration::Column::EventId.eq(event_id));
    if let Some(status) = status {
        select = select.filter(registration::Column::Status.eq(status));
    }
    select
        .order_by_asc(registration::Column::CreatedAt)
        .all(db)
        .await
        .map_err(map_db_err)
}

/// 设置报名状态。
pub async fn set_registration_status(
    db: &DatabaseConnection,
    model: &registration::Model,
    status: &str,
) -> Result<registration::Model, AppError> {
    let mut active: registration::ActiveModel = model.clone().into();
    active.status = Set(status.to_string());
    active.update(db).await.map_err(map_db_err)
}

/// 签到。
pub async fn set_checked_in(
    db: &DatabaseConnection,
    model: &registration::Model,
    now: DateTime<Utc>,
) -> Result<registration::Model, AppError> {
    let mut active: registration::ActiveModel = model.clone().into();
    active.checked_in_at = Set(Some(now.fixed_offset()));
    active.update(db).await.map_err(map_db_err)
}

/// 最早的候补记录。
pub async fn earliest_waitlist(
    db: &DatabaseConnection,
    event_id: Uuid,
) -> Result<Option<registration::Model>, AppError> {
    registration::Entity::find()
        .filter(registration::Column::EventId.eq(event_id))
        .filter(registration::Column::Status.eq("waitlist"))
        .order_by_asc(registration::Column::CreatedAt)
        .one(db)
        .await
        .map_err(map_db_err)
}

/// 写入验证码（同一活动/邮箱/用途只保留最新一条）。
pub async fn insert_verification_code(
    db: &DatabaseConnection,
    event_id: Uuid,
    email: &str,
    purpose: &str,
    code_hash: &str,
    expires_at: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Result<(), AppError> {
    registration_tokens_cleanup(db, event_id, email, purpose).await?;
    verification_code::ActiveModel {
        id: Set(new_id()),
        event_id: Set(event_id),
        email: Set(email.to_string()),
        purpose: Set(purpose.to_string()),
        code_hash: Set(code_hash.to_string()),
        expires_at: Set(expires_at.fixed_offset()),
        attempts: Set(0),
        used_at: Set(None),
        created_at: Set(now.fixed_offset()),
    }
    .insert(db)
    .await
    .map(|_| ())
    .map_err(map_db_err)
}

/// 删除旧的未使用验证码。
async fn registration_tokens_cleanup(
    db: &DatabaseConnection,
    event_id: Uuid,
    email: &str,
    purpose: &str,
) -> Result<(), AppError> {
    verification_code::Entity::delete_many()
        .filter(verification_code::Column::EventId.eq(event_id))
        .filter(verification_code::Column::Email.eq(email))
        .filter(verification_code::Column::Purpose.eq(purpose))
        .filter(verification_code::Column::UsedAt.is_null())
        .exec(db)
        .await
        .map(|_| ())
        .map_err(map_db_err)
}

/// 最新验证码记录。
pub async fn latest_verification_code(
    db: &DatabaseConnection,
    event_id: Uuid,
    email: &str,
    purpose: &str,
) -> Result<Option<verification_code::Model>, AppError> {
    verification_code::Entity::find()
        .filter(verification_code::Column::EventId.eq(event_id))
        .filter(verification_code::Column::Email.eq(email))
        .filter(verification_code::Column::Purpose.eq(purpose))
        .filter(verification_code::Column::UsedAt.is_null())
        .order_by_desc(verification_code::Column::CreatedAt)
        .one(db)
        .await
        .map_err(map_db_err)
}

/// 标记验证码已用。
pub async fn mark_code_used(
    db: &DatabaseConnection,
    model: &verification_code::Model,
    now: DateTime<Utc>,
) -> Result<(), AppError> {
    let mut active: verification_code::ActiveModel = model.clone().into();
    active.used_at = Set(Some(now.fixed_offset()));
    active.update(db).await.map(|_| ()).map_err(map_db_err)
}

/// 增加验证码尝试次数。
pub async fn bump_code_attempts(
    db: &DatabaseConnection,
    model: &verification_code::Model,
) -> Result<(), AppError> {
    verification_code::Entity::update_many()
        .col_expr(
            verification_code::Column::Attempts,
            Expr::col(verification_code::Column::Attempts).add(1),
        )
        .filter(verification_code::Column::Id.eq(model.id))
        .exec(db)
        .await
        .map(|_| ())
        .map_err(map_db_err)
}
