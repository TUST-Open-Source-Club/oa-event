//! 社团 OA 活动报名服务（event）。
//!
//! 当前进度：领域逻辑（表单 schema/答案校验、验证码、名额决策、导出表头）、
//! 实体与迁移已完成；数据访问层、HTTP 路由、Excel 导出与集成测试在下一步补齐。

#![warn(missing_docs)]

pub mod domain;
pub mod entity;
pub mod migration;
