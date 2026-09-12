//! 领域逻辑（纯函数）：表单 schema 校验、报名答案校验、验证码、状态决策。

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use club_common::{validate, AppError, FieldError};

/// 允许的表单字段类型。
pub const FIELD_TYPES: &[&str] = &[
    "text",
    "textarea",
    "number",
    "email",
    "phone",
    "select",
    "radio",
    "multiselect",
    "checkbox",
    "date",
    "file",
];

/// 需要选项的字段类型。
pub const OPTION_TYPES: &[&str] = &["select", "radio", "multiselect"];

/// 报名状态。
pub const STATUS_PENDING: &str = "pending";
/// 审核通过。
pub const STATUS_APPROVED: &str = "approved";
/// 审核拒绝。
pub const STATUS_REJECTED: &str = "rejected";
/// 候补。
pub const STATUS_WAITLIST: &str = "waitlist";
/// 取消。
pub const STATUS_CANCELLED: &str = "cancelled";

/// 校验表单 schema：字段 key 唯一、类型合法、选项类型必须有选项。
pub fn validate_form_schema(schema: &Value) -> Result<(), AppError> {
    let Some(fields) = schema.get("fields").and_then(Value::as_array) else {
        return Err(AppError::unprocessable(
            "EVENT_VALIDATION",
            "表单缺少 fields 数组",
            vec![FieldError::new("fields", "缺少")],
        ));
    };
    let mut seen = std::collections::HashSet::new();
    for field in fields {
        let key = field.get("key").and_then(Value::as_str).unwrap_or("");
        if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(AppError::unprocessable(
                "EVENT_VALIDATION",
                "字段 key 需为字母/数字/下划线",
                vec![FieldError::new("fields", "key 非法")],
            ));
        }
        if !seen.insert(key.to_string()) {
            return Err(AppError::unprocessable(
                "EVENT_VALIDATION",
                "字段 key 重复",
                vec![FieldError::new("fields", "key 重复")],
            ));
        }
        let kind = field.get("type").and_then(Value::as_str).unwrap_or("");
        if !FIELD_TYPES.contains(&kind) {
            return Err(AppError::unprocessable(
                "EVENT_VALIDATION",
                "字段类型不支持",
                vec![FieldError::new("fields", "type 非法")],
            ));
        }
        let label = field.get("label").and_then(Value::as_str).unwrap_or("");
        if label.trim().is_empty() {
            return Err(AppError::unprocessable(
                "EVENT_VALIDATION",
                "字段 label 不能为空",
                vec![FieldError::new("fields", "label 为空")],
            ));
        }
        if OPTION_TYPES.contains(&kind) {
            let options = field.get("options").and_then(Value::as_array);
            if options.map(|items| items.is_empty()).unwrap_or(true) {
                return Err(AppError::unprocessable(
                    "EVENT_VALIDATION",
                    "选项字段必须提供 options",
                    vec![FieldError::new("fields", "缺少选项")],
                ));
            }
        }
    }
    Ok(())
}

/// 姓名/邮箱等内建字段按 key 约定：`name`、`email`、`phone` 在答案中同步提取。
pub fn extract_contact(answers: &Value) -> (Option<String>, Option<String>) {
    let name = answers
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let phone = answers
        .get("phone")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    (name, phone)
}

/// 校验答案：必填、类型、选项、格式。
pub fn validate_answers(schema: &Value, answers: &Value) -> Result<(), AppError> {
    let fields = schema
        .get("fields")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut errors = Vec::new();
    for field in &fields {
        let key = field.get("key").and_then(Value::as_str).unwrap_or("");
        let kind = field.get("type").and_then(Value::as_str).unwrap_or("text");
        let required = field
            .get("required")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let value = answers.get(key);
        let is_empty = match value {
            None | Some(Value::Null) => true,
            Some(Value::String(text)) => text.trim().is_empty(),
            Some(Value::Array(items)) => items.is_empty(),
            _ => false,
        };
        if required && is_empty {
            errors.push(FieldError::new(key, "必填"));
            continue;
        }
        let Some(value) = value.filter(|_| !is_empty) else {
            continue;
        };
        match kind {
            "email" => {
                let text = value.as_str().unwrap_or("");
                if !validate::is_valid_email(text) {
                    errors.push(FieldError::new(key, "邮箱格式错误"));
                }
            }
            "number" => {
                if !value.is_number()
                    && value.as_str().and_then(|v| v.parse::<f64>().ok()).is_none()
                {
                    errors.push(FieldError::new(key, "数字格式错误"));
                }
            }
            "phone" => {
                let text = value.as_str().unwrap_or("");
                if text.chars().filter(char::is_ascii_digit).count() < 7 {
                    errors.push(FieldError::new(key, "手机号格式错误"));
                }
            }
            "select" | "radio" => {
                let options = field
                    .get("options")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let Some(text) = value.as_str() else {
                    errors.push(FieldError::new(key, "选项格式错误"));
                    continue;
                };
                let valid = options
                    .iter()
                    .any(|option| option.get("value").and_then(Value::as_str) == Some(text));
                if !valid {
                    errors.push(FieldError::new(key, "选项不在允许范围"));
                }
            }
            "multiselect" => {
                let options = field
                    .get("options")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let Some(items) = value.as_array() else {
                    errors.push(FieldError::new(key, "多选格式错误"));
                    continue;
                };
                for item in items {
                    let text = item.as_str().unwrap_or("");
                    if !options
                        .iter()
                        .any(|option| option.get("value").and_then(Value::as_str) == Some(text))
                    {
                        errors.push(FieldError::new(key, "选项不在允许范围"));
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(AppError::unprocessable(
            "EVENT_VALIDATION",
            "报名信息校验失败",
            errors,
        ))
    }
}

/// 邮箱域名白名单（空 = 不限制）。
pub fn email_allowed(email: &str, domains: &[String]) -> bool {
    validate::email_domain_allowed(email, domains)
}

/// 生成 6 位数字验证码。
pub fn generate_code() -> String {
    let value: u32 = rand::random::<u32>() % 1_000_000;
    format!("{value:06}")
}

/// 验证码哈希（SHA-256 hex）。
pub fn hash_code(code: &str) -> String {
    hex::encode(Sha256::digest(code.as_bytes()))
}

/// 生成签到码（24 hex）。
pub fn generate_checkin_code() -> String {
    let bytes: [u8; 12] = rand::random();
    hex::encode(bytes)
}

/// 报名状态决策。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// 直接通过。
    Approved,
    /// 待审核。
    Pending,
    /// 进入候补。
    Waitlist,
}

/// 根据名额/候补/审核开关决定初始状态。
pub fn registration_decision(
    approved_count: i64,
    capacity: i64,
    waitlist_enabled: bool,
    need_review: bool,
) -> Decision {
    let full = capacity > 0 && approved_count >= capacity;
    if full && waitlist_enabled {
        return Decision::Waitlist;
    }
    if full {
        return Decision::Approved; // 逻辑上不应发生，路由层先拦截满员
    }
    if need_review {
        Decision::Pending
    } else {
        Decision::Approved
    }
}

/// Excel 导出表头（内建列 + 自定义字段 label）。
pub fn export_headers(schema: &Value) -> Vec<String> {
    let mut headers = vec![
        "序号".to_string(),
        "姓名".to_string(),
        "邮箱".to_string(),
        "手机".to_string(),
        "状态".to_string(),
        "已签到".to_string(),
        "报名时间".to_string(),
    ];
    if let Some(fields) = schema.get("fields").and_then(Value::as_array) {
        for field in fields {
            let key = field.get("key").and_then(Value::as_str).unwrap_or("");
            if matches!(key, "name" | "email" | "phone") {
                continue;
            }
            let label = field.get("label").and_then(Value::as_str).unwrap_or(key);
            headers.push(label.to_string());
        }
    }
    headers
}

/// 状态中文名（导出用）。
pub fn status_label(status: &str) -> &'static str {
    match status {
        STATUS_APPROVED => "已通过",
        STATUS_PENDING => "待审核",
        STATUS_REJECTED => "已拒绝",
        STATUS_WAITLIST => "候补中",
        STATUS_CANCELLED => "已取消",
        _ => "未知",
    }
}

/// 构造默认姓名/邮箱字段 schema（便于管理端快速开始）。
pub fn default_schema() -> Value {
    json!({
        "version": 1,
        "fields": [
            { "key": "name", "type": "text", "label": "姓名", "required": true, "maxLength": 50 },
            { "key": "email", "type": "email", "label": "邮箱", "required": true },
            { "key": "phone", "type": "phone", "label": "手机号", "required": false },
            { "key": "note", "type": "textarea", "label": "备注", "required": false }
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn form_schema_validation() {
        assert!(validate_form_schema(&default_schema()).is_ok());
        // 缺 fields
        assert!(validate_form_schema(&json!({})).is_err());
        // 重复 key
        let dup = json!({"fields": [
            {"key": "a", "type": "text", "label": "A"},
            {"key": "a", "type": "text", "label": "B"}
        ]});
        assert!(validate_form_schema(&dup).is_err());
        // 选项字段无选项
        let no_options = json!({"fields": [{"key": "x", "type": "select", "label": "X"}]});
        assert!(validate_form_schema(&no_options).is_err());
        // 未知类型
        let bad_type = json!({"fields": [{"key": "x", "type": "sheet", "label": "X"}]});
        assert!(validate_form_schema(&bad_type).is_err());
    }

    #[test]
    fn answers_validation() {
        let schema = json!({"fields": [
            {"key": "name", "type": "text", "label": "姓名", "required": true},
            {"key": "email", "type": "email", "label": "邮箱", "required": true},
            {"key": "diet", "type": "select", "label": "饮食", "required": false,
             "options": [{"label": "无", "value": "none"}, {"label": "素食", "value": "veg"}]}
        ]});
        assert!(validate_answers(
            &schema,
            &json!({"name": "张三", "email": "a@b.cn", "diet": "veg"})
        )
        .is_ok());
        assert!(
            validate_answers(&schema, &json!({"email": "a@b.cn"})).is_err(),
            "姓名必填"
        );
        assert!(validate_answers(&schema, &json!({"name": "张三", "email": "bad"})).is_err());
        assert!(validate_answers(
            &schema,
            &json!({"name": "张三", "email": "a@b.cn", "diet": "meat"})
        )
        .is_err());
    }

    #[test]
    fn decisions_cover_capacity_and_review() {
        assert_eq!(
            registration_decision(0, 10, false, false),
            Decision::Approved
        );
        assert_eq!(registration_decision(0, 10, false, true), Decision::Pending);
        assert_eq!(
            registration_decision(10, 10, true, false),
            Decision::Waitlist
        );
        assert_eq!(
            registration_decision(0, 0, false, false),
            Decision::Approved,
            "capacity=0 不限"
        );
    }

    #[test]
    fn codes_and_contact_extraction() {
        let code = generate_code();
        assert_eq!(code.len(), 6);
        assert!(code.chars().all(|c| c.is_ascii_digit()));
        assert_eq!(hash_code("123456"), hash_code("123456"));
        assert_ne!(hash_code("123456"), hash_code("123457"));
        assert_eq!(generate_checkin_code().len(), 24);
        let (name, phone) = extract_contact(&json!({"name": " 李四 ", "phone": "13800000000"}));
        assert_eq!(name.as_deref(), Some("李四"));
        assert_eq!(phone.as_deref(), Some("13800000000"));
    }

    #[test]
    fn export_headers_skip_contact_fields() {
        let headers = export_headers(&default_schema());
        assert_eq!(headers[0], "序号");
        assert!(headers.contains(&"备注".to_string()));
        assert!(
            !headers.contains(&"姓名".to_string())
                || headers.iter().filter(|h| *h == "姓名").count() == 1
        );
    }
}
