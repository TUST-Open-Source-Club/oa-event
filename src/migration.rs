//! event schema 迁移（含 outbox）。

use sea_orm_migration::prelude::*;

/// 初始化迁移。
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("CREATE SCHEMA IF NOT EXISTS event")
            .await?;
        manager
            .get_connection()
            .execute_unprepared(club_bus::outbox::DDL)
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Events::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Events::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Events::Slug).string_len(64).not_null())
                    .col(ColumnDef::new(Events::Title).string_len(200).not_null())
                    .col(
                        ColumnDef::new(Events::DescriptionMd)
                            .text()
                            .not_null()
                            .default(""),
                    )
                    .col(ColumnDef::new(Events::Location).string_len(200))
                    .col(ColumnDef::new(Events::StartAt).timestamp_with_time_zone())
                    .col(ColumnDef::new(Events::EndAt).timestamp_with_time_zone())
                    .col(ColumnDef::new(Events::RegStartAt).timestamp_with_time_zone())
                    .col(ColumnDef::new(Events::RegEndAt).timestamp_with_time_zone())
                    .col(
                        ColumnDef::new(Events::Capacity)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Events::WaitlistEnabled)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .col(
                        ColumnDef::new(Events::NeedReview)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(Events::EmailVerify)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .col(
                        ColumnDef::new(Events::EmailDomains)
                            .json_binary()
                            .not_null()
                            .default("[]"),
                    )
                    .col(
                        ColumnDef::new(Events::Status)
                            .string_len(16)
                            .not_null()
                            .default("draft"),
                    )
                    .col(ColumnDef::new(Events::CreatedBy).uuid().not_null())
                    .col(
                        ColumnDef::new(Events::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Events::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("ux_events_slug")
                    .table(Events::Table)
                    .col(Events::Slug)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(EventAdmins::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(EventAdmins::EventId).uuid().not_null())
                    .col(ColumnDef::new(EventAdmins::UserId).uuid().not_null())
                    .col(
                        ColumnDef::new(EventAdmins::JoinedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .primary_key(
                        Index::create()
                            .col(EventAdmins::EventId)
                            .col(EventAdmins::UserId),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(FormSchemas::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(FormSchemas::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(FormSchemas::EventId).uuid().not_null())
                    .col(
                        ColumnDef::new(FormSchemas::Version)
                            .big_integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(FormSchemas::Schema).json_binary().not_null())
                    .col(
                        ColumnDef::new(FormSchemas::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("ux_form_schemas_event_version")
                    .table(FormSchemas::Table)
                    .col(FormSchemas::EventId)
                    .col(FormSchemas::Version)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Registrations::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Registrations::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Registrations::EventId).uuid().not_null())
                    .col(
                        ColumnDef::new(Registrations::SchemaVersion)
                            .big_integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(Registrations::UserId).uuid())
                    .col(ColumnDef::new(Registrations::Name).string_len(100))
                    .col(
                        ColumnDef::new(Registrations::Email)
                            .string_len(254)
                            .not_null(),
                    )
                    .col(ColumnDef::new(Registrations::Phone).string_len(32))
                    .col(
                        ColumnDef::new(Registrations::Answers)
                            .json_binary()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Registrations::Status)
                            .string_len(16)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Registrations::CheckinCode)
                            .string_len(32)
                            .not_null(),
                    )
                    .col(ColumnDef::new(Registrations::CheckedInAt).timestamp_with_time_zone())
                    .col(
                        ColumnDef::new(Registrations::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("ux_registrations_checkin")
                    .table(Registrations::Table)
                    .col(Registrations::CheckinCode)
                    .unique()
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("ix_registrations_event_status")
                    .table(Registrations::Table)
                    .col(Registrations::EventId)
                    .col(Registrations::Status)
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(VerificationCodes::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(VerificationCodes::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(VerificationCodes::EventId).uuid().not_null())
                    .col(
                        ColumnDef::new(VerificationCodes::Email)
                            .string_len(254)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(VerificationCodes::Purpose)
                            .string_len(16)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(VerificationCodes::CodeHash)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(VerificationCodes::ExpiresAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(VerificationCodes::Attempts)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(ColumnDef::new(VerificationCodes::UsedAt).timestamp_with_time_zone())
                    .col(
                        ColumnDef::new(VerificationCodes::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("ix_verification_codes_lookup")
                    .table(VerificationCodes::Table)
                    .col(VerificationCodes::EventId)
                    .col(VerificationCodes::Email)
                    .col(VerificationCodes::Purpose)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in [
            "verification_codes",
            "registrations",
            "form_schemas",
            "event_admins",
            "events",
        ] {
            manager
                .get_connection()
                .execute_unprepared(&format!("DROP TABLE IF EXISTS {table}"))
                .await?;
        }
        Ok(())
    }
}

/// 迁移入口。
pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(Migration),
            Box::new(crate::migration2::Migration),
        ]
    }
}

/// events 表标识符。
#[derive(DeriveIden)]
pub enum Events {
    /// 表。
    Table,
    /// id。
    Id,
    /// slug。
    Slug,
    /// title。
    Title,
    /// description_md。
    DescriptionMd,
    /// location。
    Location,
    /// start_at。
    StartAt,
    /// end_at。
    EndAt,
    /// reg_start_at。
    RegStartAt,
    /// reg_end_at。
    RegEndAt,
    /// capacity。
    Capacity,
    /// waitlist_enabled。
    WaitlistEnabled,
    /// need_review。
    NeedReview,
    /// email_verify。
    EmailVerify,
    /// email_domains。
    EmailDomains,
    /// status。
    Status,
    /// created_by。
    CreatedBy,
    /// created_at。
    CreatedAt,
    /// updated_at。
    UpdatedAt,
}

/// event_admins 表标识符。
#[derive(DeriveIden)]
pub enum EventAdmins {
    /// 表。
    Table,
    /// event_id。
    EventId,
    /// user_id。
    UserId,
    /// joined_at。
    JoinedAt,
}

/// form_schemas 表标识符。
#[derive(DeriveIden)]
pub enum FormSchemas {
    /// 表。
    Table,
    /// id。
    Id,
    /// event_id。
    EventId,
    /// version。
    Version,
    /// schema。
    Schema,
    /// created_at。
    CreatedAt,
}

/// registrations 表标识符。
#[derive(DeriveIden)]
pub enum Registrations {
    /// 表。
    Table,
    /// id。
    Id,
    /// event_id。
    EventId,
    /// schema_version。
    SchemaVersion,
    /// user_id。
    UserId,
    /// name。
    Name,
    /// email。
    Email,
    /// phone。
    Phone,
    /// answers。
    Answers,
    /// status。
    Status,
    /// checkin_code。
    CheckinCode,
    /// checked_in_at。
    CheckedInAt,
    /// created_at。
    CreatedAt,
}

/// verification_codes 表标识符。
#[derive(DeriveIden)]
pub enum VerificationCodes {
    /// 表。
    Table,
    /// id。
    Id,
    /// event_id。
    EventId,
    /// email。
    Email,
    /// purpose。
    Purpose,
    /// code_hash。
    CodeHash,
    /// expires_at。
    ExpiresAt,
    /// attempts。
    Attempts,
    /// used_at。
    UsedAt,
    /// created_at。
    CreatedAt,
}
