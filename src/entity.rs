//! event schema 实体。

/// 活动。
pub mod event {
    use sea_orm::entity::prelude::*;

    /// 草稿。
    pub const STATUS_DRAFT: &str = "draft";
    /// 开放报名。
    pub const STATUS_OPEN: &str = "open";
    /// 已关闭。
    pub const STATUS_CLOSED: &str = "closed";

    /// 活动模型。
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "events")]
    pub struct Model {
        /// ID。
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        /// URL slug（唯一）。
        pub slug: String,
        /// 标题。
        pub title: String,
        /// 详情（Markdown）。
        #[sea_orm(column_type = "Text")]
        pub description_md: String,
        /// 地点。
        #[sea_orm(nullable)]
        pub location: Option<String>,
        /// 开始时间。
        #[sea_orm(nullable)]
        pub start_at: Option<DateTimeWithTimeZone>,
        /// 结束时间。
        #[sea_orm(nullable)]
        pub end_at: Option<DateTimeWithTimeZone>,
        /// 报名开始。
        #[sea_orm(nullable)]
        pub reg_start_at: Option<DateTimeWithTimeZone>,
        /// 报名截止。
        #[sea_orm(nullable)]
        pub reg_end_at: Option<DateTimeWithTimeZone>,
        /// 名额（0 = 不限）。
        pub capacity: i64,
        /// 是否启用候补。
        pub waitlist_enabled: bool,
        /// 是否需要审核。
        pub need_review: bool,
        /// 是否要求邮箱验证码。
        pub email_verify: bool,
        /// 邮箱域名白名单（空数组 = 不限制）。
        #[sea_orm(column_type = "JsonBinary")]
        pub email_domains: Json,
        /// draft / open / closed。
        pub status: String,
        /// 创建者。
        pub created_by: Uuid,
        /// 创建时间。
        pub created_at: DateTimeWithTimeZone,
        /// 更新时间。
        pub updated_at: DateTimeWithTimeZone,
    }

    /// 关系。
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// 活动管理员。
pub mod event_admin {
    use sea_orm::entity::prelude::*;

    /// 管理员模型。
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "event_admins")]
    pub struct Model {
        /// 活动。
        #[sea_orm(primary_key, auto_increment = false)]
        pub event_id: Uuid,
        /// 用户。
        #[sea_orm(primary_key, auto_increment = false)]
        pub user_id: Uuid,
        /// 加入时间。
        pub joined_at: DateTimeWithTimeZone,
    }

    /// 关系。
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// 表单 schema 版本。
pub mod form_schema {
    use sea_orm::entity::prelude::*;

    /// 模型。
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "form_schemas")]
    pub struct Model {
        /// ID。
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        /// 活动。
        pub event_id: Uuid,
        /// 版本号。
        pub version: i64,
        /// schema JSON。
        #[sea_orm(column_type = "JsonBinary")]
        pub schema: Json,
        /// 创建时间。
        pub created_at: DateTimeWithTimeZone,
    }

    /// 关系。
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// 报名记录。
pub mod registration {
    use sea_orm::entity::prelude::*;

    /// 模型。
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "registrations")]
    pub struct Model {
        /// ID。
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        /// 活动。
        pub event_id: Uuid,
        /// 表单版本。
        pub schema_version: i64,
        /// 注册用户（游客为空）。
        #[sea_orm(nullable)]
        pub user_id: Option<Uuid>,
        /// 姓名。
        #[sea_orm(nullable)]
        pub name: Option<String>,
        /// 邮箱。
        pub email: String,
        /// 手机。
        #[sea_orm(nullable)]
        pub phone: Option<String>,
        /// 答案 JSON。
        #[sea_orm(column_type = "JsonBinary")]
        pub answers: Json,
        /// 状态。
        pub status: String,
        /// 签到码（唯一）。
        pub checkin_code: String,
        /// 签到时间。
        #[sea_orm(nullable)]
        pub checked_in_at: Option<DateTimeWithTimeZone>,
        /// 创建时间。
        pub created_at: DateTimeWithTimeZone,
    }

    /// 关系。
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// 邮箱验证码。
pub mod verification_code {
    use sea_orm::entity::prelude::*;

    /// 用途：报名。
    pub const PURPOSE_REGISTER: &str = "register";

    /// 模型。
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "verification_codes")]
    pub struct Model {
        /// ID。
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        /// 活动。
        pub event_id: Uuid,
        /// 邮箱。
        pub email: String,
        /// 用途。
        pub purpose: String,
        /// 验证码哈希。
        pub code_hash: String,
        /// 过期时间。
        pub expires_at: DateTimeWithTimeZone,
        /// 尝试次数。
        pub attempts: i32,
        /// 使用时间。
        #[sea_orm(nullable)]
        pub used_at: Option<DateTimeWithTimeZone>,
        /// 创建时间。
        pub created_at: DateTimeWithTimeZone,
    }

    /// 关系。
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
